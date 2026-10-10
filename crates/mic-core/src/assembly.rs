use std::collections::{BTreeSet, HashMap};
use std::fs::{File, TryLockError};
use std::path::PathBuf;
use std::sync::Arc;

use mic_message::{PersonId, SessionId};
use mic_store::{
    InputDisposition, Migration, NewSession, RunState, SecretKeyFile, SessionKind, Store, ToolScope,
};
use mic_tool::ToolHandle;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::task::{Id, JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::event::{Events, KernelEvent};
use crate::input::validate;
use crate::limits::WAKE_CHANNEL_CAPACITY;
use crate::provider::Factories;
use crate::run::{now_ms, Engine};
use crate::scheduler::panic_message;
use crate::{
    kernel, recovery, scheduler, Activation, AssembleError, BoxError, ChannelSetup, IncomingPart,
    Kernel, Module, ModuleConfig, Registry, RunError, Service,
};

const DEFAULT_OWNER: &str = "dzmfg";
const ONCE_CHANNEL: &str = "cli";

/// 装配入口，由二进制调用。
pub struct Assembly {
    data_dir: PathBuf,
    migrations: Vec<Migration>,
    services: Vec<(&'static str, Box<dyn Service>)>,
    owner: String,
    key_file: SecretKeyFile,
    factories: Arc<Factories>,
    tools: Vec<ToolHandle>,
    channel_setups: Arc<HashMap<&'static str, Arc<dyn ChannelSetup>>>,
    channel_prompts: Arc<HashMap<&'static str, &'static str>>,
}

/// `-p` 的一次性输入。
pub struct OneShot {
    pub prompt: String,
    /// 会话工作目录，UTF-8 绝对路径（二进制取进程 cwd 并在入口校验）。
    pub pwd: PathBuf,
}

pub enum OnceOutcome {
    Finished {
        session_id: SessionId,
        state: RunState,
    },
    /// 收到 `stop` 时本轮未结束；该 run 留在 `Executing`，下次启动收尾为 `Interrupted`。
    Stopped { session_id: SessionId },
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct CoreConfig {
    data_dir: Option<String>,
    owner: Option<String>,
    /// 模型 API key 的主密钥文件；二进制按配置文件位置补缺省值。
    key_file: Option<String>,
}

/// `run` 与 `run_once` 共用的启动结果；`_lock` 持有期间独占数据目录。
struct Started {
    store: Store,
    owner: PersonId,
    _lock: File,
}

impl Assembly {
    pub fn new(
        modules: Vec<Box<dyn Module>>,
        mut config: toml::Table,
    ) -> Result<Self, AssembleError> {
        let mut names = BTreeSet::new();
        for m in &modules {
            if !names.insert(m.name()) {
                return Err(AssembleError::DuplicateModule { name: m.name() });
            }
        }

        if config.remove("models").is_some() {
            return Err(AssembleError::RemovedModelsConfig);
        }
        if let Some(name) = config
            .keys()
            .find(|k| *k != "core" && !names.contains(k.as_str()))
        {
            return Err(AssembleError::UnknownModule { name: name.clone() });
        }

        let core = match config.remove("core") {
            Some(v) if v.get("max_turns").is_some() => {
                return Err(AssembleError::Core {
                    source: "max_turns 已移到网页 设置 → 对话偏好，请从配置删掉这一行".into(),
                })
            }
            Some(v) => v
                .try_into::<CoreConfig>()
                .map_err(|e| AssembleError::Core { source: e.into() })?,
            None => CoreConfig::default(),
        };
        let data_dir =
            resolve_data_dir(core.data_dir).map_err(|source| AssembleError::Core { source })?;
        let key_file = core
            .key_file
            .ok_or_else(|| AssembleError::Core {
                source: "缺少 key_file（主密钥文件路径）".into(),
            })
            .and_then(|path| {
                resolve_abs("key_file", &path).map_err(|source| AssembleError::Core { source })
            })?;
        let owner = core.owner.unwrap_or_else(|| DEFAULT_OWNER.to_owned());
        if owner.contains(':') {
            return Err(AssembleError::Core {
                source: format!("owner 不得含 \":\"，当前为 \"{owner}\"").into(),
            });
        }

        let mut reg = Registry {
            current: "",
            migrations: Vec::new(),
            services: Vec::new(),
            providers: Vec::new(),
            tools: Vec::new(),
            channel_setups: Vec::new(),
            channel_prompts: Vec::new(),
        };
        for m in &modules {
            let module = m.name();
            if let Some(section) = config.remove(module) {
                if !section.is_table() {
                    return Err(AssembleError::Install {
                        module,
                        source: format!("[{module}] 必须是一个段（表），不能是单个值").into(),
                    });
                }
                install(m.as_ref(), &mut reg, section)?;
            } else if m.activation() == Activation::Always {
                install(m.as_ref(), &mut reg, toml::Value::Table(toml::Table::new()))?;
            }
        }

        let mut owners: HashMap<&str, &'static str> = HashMap::new();
        for (module, tool) in &reg.tools {
            if let Some(first) = owners.insert(tool.name(), module) {
                return Err(AssembleError::DuplicateTool {
                    name: tool.name().to_owned(),
                    first,
                    second: module,
                });
            }
        }

        let mut factories = Factories::default();
        for factory in reg.providers {
            factories
                .insert(factory)
                .map_err(|kind| AssembleError::DuplicateProviderKind { kind })?;
        }

        let mut channel_setups = HashMap::new();
        for (channel, setup) in reg.channel_setups {
            if channel_setups.insert(channel, setup).is_some() {
                return Err(AssembleError::DuplicateChannelSetup { channel });
            }
        }

        let mut channel_prompts = HashMap::new();
        for (channel, prompt) in reg.channel_prompts {
            if channel_prompts.insert(channel, prompt).is_some() {
                return Err(AssembleError::DuplicateChannelPrompt { channel });
            }
        }

        Ok(Self {
            data_dir,
            migrations: reg.migrations,
            services: reg.services,
            owner,
            key_file: SecretKeyFile(key_file),
            factories: Arc::new(factories),
            tools: reg.tools.into_iter().map(|(_, t)| t).collect(),
            channel_setups: Arc::new(channel_setups),
            channel_prompts: Arc::new(channel_prompts),
        })
    }

    /// 常驻：独占数据目录、打开 Store、崩溃收尾、起调度与各 Service，直到 `stop` 或出错。
    pub async fn run(mut self, stop: CancellationToken) -> Result<(), RunError> {
        let started = self.start().await?;
        let services = std::mem::take(&mut self.services);
        let events = Events::new();
        let (wake_tx, wake_rx) = mpsc::channel(WAKE_CHANNEL_CAPACITY);
        let kernel = Kernel::new(
            started.store.clone(),
            started.owner,
            events.clone(),
            wake_tx,
            self.factories.clone(),
            self.channel_setups.clone(),
        );
        let engine = Arc::new(self.engine(started.store.clone(), events));
        let mut scheduler = Box::pin(scheduler::run(engine, wake_rx));

        let shutdown = stop.child_token();
        let mut tasks = JoinSet::new();
        let mut owners = HashMap::new();
        for (module, service) in services {
            let handle = tasks.spawn(service.run(kernel.clone(), shutdown.clone()));
            owners.insert(handle.id(), module);
        }

        let mut failure = None;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                error = &mut scheduler => {
                    shutdown.cancel();
                    failure = Some(error);
                    break;
                }
                Some(joined) = tasks.join_next_with_id() => {
                    settle(joined, &owners, &shutdown, &mut failure);
                }
            }
        }
        // 先停执行（进行中的 run 留在 Executing，下次启动收尾），再等 Service 退出。
        drop(scheduler);
        while let Some(joined) = tasks.join_next_with_id().await {
            settle(joined, &owners, &shutdown, &mut failure);
        }
        failure.map_or(Ok(()), Err)
    }

    /// 一次性：同样的启动与收尾，但不起 Service、不补跑其它会话；新建 `cli` 会话写入 `prompt`，
    /// 跑完这一轮即返回。事件经 `on_event` 交给调用方呈现。
    pub async fn run_once(
        mut self,
        once: OneShot,
        mut on_event: impl FnMut(&KernelEvent) + Send,
        stop: CancellationToken,
    ) -> Result<OnceOutcome, RunError> {
        let started = self.start().await?;
        let store = started.store.clone();
        let events = Events::new();
        let mut rx = events.subscribe();
        let now = now_ms();
        let chat = now.to_string();
        let session = store
            .resolve_root_session(
                ONCE_CHANNEL,
                &chat,
                NewSession {
                    kind: SessionKind::Root {
                        channel: ONCE_CHANNEL.to_owned(),
                        chat: chat.clone(),
                    },
                    parent_session_id: None,
                    delivery_target: None,
                    pwd: once
                        .pwd
                        .into_os_string()
                        .into_string()
                        .expect("OneShot.pwd 由调用方保证是 UTF-8"),
                    tool_scope: ToolScope::All,
                    created_at: now,
                    persona: None,
                    model: None,
                },
            )
            .await?;
        let session_id = session.id;
        kernel::append_input(
            &store,
            &events,
            session_id,
            started.owner,
            validate(vec![IncomingPart::Text(once.prompt)]).await?,
            InputDisposition::Pending,
            None,
        )
        .await?;
        let claimed = store
            .claim_next(session_id, now_ms())
            .await?
            .expect("刚写入的输入必然可认领");

        let engine = Arc::new(self.engine(store, events));
        let mut task = JoinSet::new();
        task.spawn(async move { engine.run(&session, ONCE_CHANNEL, claimed).await });
        let mut show = |event: Result<KernelEvent, crate::Lagged>| match event {
            Ok(e) if e.session_id == session_id => on_event(&e),
            Ok(_) => {}
            Err(lagged) => tracing::warn!(%lagged, "event display lagged"),
        };
        loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => return Ok(OnceOutcome::Stopped { session_id }),
                event = rx.recv() => show(event),
                Some(joined) = task.join_next() => {
                    while let Some(event) = rx.try_recv() {
                        show(event);
                    }
                    let state = joined.map_err(|e| RunError::RunPanicked {
                        session_id,
                        message: panic_message(e),
                    })??;
                    return Ok(OnceOutcome::Finished { session_id, state });
                }
            }
        }
    }

    /// 建数据目录、加锁、打开 Store、崩溃收尾、确保 owner。
    async fn start(&mut self) -> Result<Started, RunError> {
        let data_dir = &self.data_dir;
        std::fs::create_dir_all(data_dir).map_err(|source| RunError::DataDir {
            path: data_dir.clone(),
            source,
        })?;
        let lock_path = data_dir.join("micnext.lock");
        let lock = File::create(&lock_path).map_err(|source| RunError::DataDir {
            path: lock_path.clone(),
            source,
        })?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(RunError::DataDirLocked {
                    path: data_dir.clone(),
                })
            }
            Err(TryLockError::Error(source)) => {
                return Err(RunError::DataDir {
                    path: lock_path,
                    source,
                })
            }
        }
        let db = data_dir.join("micnext.db");
        let store = Store::open(
            &db,
            &std::mem::take(&mut self.migrations),
            self.key_file.clone(),
        )
        .await?;
        let interrupted = recovery::recover(&store).await?;
        let held_sessions = store.hold_unclaimed_inputs(now_ms()).await?.len();
        let owner = store.ensure_person(&self.owner, now_ms()).await?;
        tracing::info!(db = %db.display(), interrupted, held_sessions, "store ready");
        Ok(Started {
            store,
            owner,
            _lock: lock,
        })
    }

    fn engine(&self, store: Store, events: Events) -> Engine {
        Engine {
            store,
            events,
            factories: self.factories.clone(),
            tools: self.tools.clone(),
            channel_prompts: self.channel_prompts.clone(),
        }
    }
}

/// 结算一个已结束的 Service；失败即广播 stop，只保留第一个错误，其余记日志。
fn settle(
    joined: Result<(Id, Result<(), BoxError>), JoinError>,
    owners: &HashMap<Id, &'static str>,
    shutdown: &CancellationToken,
    failure: &mut Option<RunError>,
) {
    let (id, result) = match joined {
        Ok((id, result)) => (id, result),
        Err(e) => (e.id(), Err(Box::new(e) as BoxError)),
    };
    let module = owners[&id];
    let error = match result {
        Ok(()) if shutdown.is_cancelled() => return,
        Ok(()) => RunError::ServiceExited { module },
        Err(source) => RunError::Service { module, source },
    };
    shutdown.cancel();
    match failure {
        None => *failure = Some(error),
        Some(_) => tracing::error!(%error, "additional service failure"),
    }
}

fn install(m: &dyn Module, reg: &mut Registry, cfg: toml::Value) -> Result<(), AssembleError> {
    let module = m.name();
    reg.current = module;
    m.install(reg, ModuleConfig(cfg))
        .map_err(|source| AssembleError::Install { module, source })
}

/// `[core] data_dir`（绝对路径或 `~/` 开头），缺省按 XDG：`$XDG_DATA_HOME/micnext` 或 `~/.local/share/micnext`。
fn resolve_data_dir(configured: Option<String>) -> Result<PathBuf, BoxError> {
    if let Some(dir) = configured {
        return match dir.strip_prefix("~/") {
            Some(rest) => Ok(home()?.join(rest)),
            None if dir.starts_with('/') => Ok(PathBuf::from(dir)),
            None => Err(format!("data_dir 必须是绝对路径或以 ~/ 开头，当前为 \"{dir}\"").into()),
        };
    }
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => home()?.join(".local/share"),
    };
    Ok(base.join("micnext"))
}

/// 绝对路径或 `~/` 开头。
fn resolve_abs(field: &str, path: &str) -> Result<PathBuf, BoxError> {
    match path.strip_prefix("~/") {
        Some(rest) => Ok(home()?.join(rest)),
        None if path.starts_with('/') => Ok(PathBuf::from(path)),
        None => Err(format!("{field} 必须是绝对路径或以 ~/ 开头，当前为 \"{path}\"").into()),
    }
}

fn home() -> Result<PathBuf, BoxError> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "环境变量 HOME 未设置".into())
}
