use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use mic_store::{Migration, Store};
use serde::Deserialize;
use tokio::task::{Id, JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::{AssembleError, BoxError, Kernel, Module, ModuleConfig, Registry, RunError, Service};

/// 装配入口，由二进制调用。
pub struct Assembly {
    data_dir: PathBuf,
    migrations: Vec<Migration>,
    services: Vec<(&'static str, Box<dyn Service>)>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct CoreConfig {
    data_dir: Option<String>,
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
        if let Some(name) = config
            .keys()
            .find(|k| *k != "core" && !names.contains(k.as_str()))
        {
            return Err(AssembleError::UnknownModule { name: name.clone() });
        }

        let core = match config.remove("core") {
            Some(v) => v
                .try_into::<CoreConfig>()
                .map_err(|e| AssembleError::Core { source: e.into() })?,
            None => CoreConfig::default(),
        };
        let data_dir =
            resolve_data_dir(core.data_dir).map_err(|source| AssembleError::Core { source })?;

        let mut reg = Registry {
            current: "",
            migrations: Vec::new(),
            services: Vec::new(),
        };
        for m in &modules {
            let module = m.name();
            let Some(section) = config.remove(module) else {
                continue;
            };
            if !section.is_table() {
                return Err(AssembleError::Install {
                    module,
                    source: format!("[{module}] 必须是一个段（表），不能是单个值").into(),
                });
            }
            reg.current = module;
            m.install(&mut reg, ModuleConfig(section))
                .map_err(|source| AssembleError::Install { module, source })?;
        }

        Ok(Self {
            data_dir,
            migrations: reg.migrations,
            services: reg.services,
        })
    }

    /// 打开 Store、收尾遗留 Query、启动 Service，直到 `stop` 或任一 Service 失败。
    pub async fn run(self, stop: CancellationToken) -> Result<(), RunError> {
        std::fs::create_dir_all(&self.data_dir).map_err(|source| RunError::DataDir {
            path: self.data_dir.clone(),
            source,
        })?;
        let db = self.data_dir.join("micnext.db");
        let store = Store::open(&db, &self.migrations).await?;
        let interrupted = store.interrupt_stale_queries(now_ms()).await?;
        tracing::info!(db = %db.display(), interrupted = interrupted.len(), "store ready");

        let kernel = Kernel::new(store);
        let shutdown = stop.child_token();
        let mut tasks = JoinSet::new();
        let mut owners = HashMap::new();
        for (module, service) in self.services {
            let handle = tasks.spawn(service.run(kernel.clone(), shutdown.clone()));
            owners.insert(handle.id(), module);
        }

        let mut failure = None;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                Some(joined) = tasks.join_next_with_id() => {
                    settle(joined, &owners, &shutdown, &mut failure);
                }
            }
        }
        while let Some(joined) = tasks.join_next_with_id().await {
            settle(joined, &owners, &shutdown, &mut failure);
        }
        failure.map_or(Ok(()), Err)
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

fn home() -> Result<PathBuf, BoxError> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "环境变量 HOME 未设置".into())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟早于 1970")
        .as_millis() as i64
}
