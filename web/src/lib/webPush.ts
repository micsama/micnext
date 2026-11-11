import { deleteWebPushSubscription, getWebPushKey, putWebPushSubscription } from "../api/client";

/** iPhone 只有从主屏幕打开的应用才提供 PushManager。 */
export const webPushSupported =
  typeof window !== "undefined" && "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;

async function registration(): Promise<ServiceWorkerRegistration> {
  await navigator.serviceWorker.register("/sw.js");
  return navigator.serviceWorker.ready;
}

export async function currentSubscription(): Promise<PushSubscription | null> {
  return (await registration()).pushManager.getSubscription();
}

function decodeKey(key: string): Uint8Array<ArrayBuffer> {
  const raw = atob(key.replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

export async function enableWebPush(): Promise<void> {
  if ((await Notification.requestPermission()) !== "granted") {
    throw new Error("系统通知未获授权：去系统设置里允许 micnext 发送通知");
  }
  const reg = await registration();
  const sub =
    (await reg.pushManager.getSubscription()) ??
    (await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: decodeKey(await getWebPushKey()) }));
  await putWebPushSubscription(sub.toJSON());
}

export async function disableWebPush(sub: PushSubscription): Promise<void> {
  await sub.unsubscribe();
  await deleteWebPushSubscription(sub.endpoint);
}
