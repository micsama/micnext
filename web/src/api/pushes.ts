import { arr, bool, kinded, nullable, num, obj, str, type Decoder } from "./decode";

// 与 docs/blueprints/scheduled-tasks.md P1 一一对应。

export type PushDeliveryState =
  | { kind: "shown" }
  | { kind: "pending" }
  | { kind: "delivered"; at: number }
  | { kind: "failed"; reason: string };

export type PushDelivery = { channel: string; state: PushDeliveryState };

export type PushView = {
  id: number;
  source_session_id: number;
  source_session_title: string | null;
  text: string;
  created_at: number;
  /** 没有发往 Web 时恒为 null。 */
  read_at: number | null;
  deliveries: PushDelivery[];
};

export type PushPage = { pushes: PushView[]; has_more: boolean; unread: number };

export type PushSettings = { channels: string[]; chosen: string[]; effective: string[] };

const delivery: Decoder<PushDelivery> = obj({
  channel: str,
  state: kinded<{
    shown: Record<string, never>;
    pending: Record<string, never>;
    delivered: { at: number };
    failed: { reason: string };
  }>({ shown: {}, pending: {}, delivered: { at: num }, failed: { reason: str } }),
});

const pushPage: Decoder<PushPage> = obj({
  pushes: arr(
    obj({
      id: num,
      source_session_id: num,
      source_session_title: nullable(str),
      text: str,
      created_at: num,
      read_at: nullable(num),
      deliveries: arr(delivery),
    }),
  ),
  has_more: bool,
  unread: num,
});

const pushSettings: Decoder<PushSettings> = obj({ channels: arr(str), chosen: arr(str), effective: arr(str) });

export const parsePushPage = (v: unknown): PushPage => pushPage(v, "推送列表");
export const parsePushSettings = (v: unknown): PushSettings => pushSettings(v, "推送设置");
export const parsePushCreated = (data: string): number => obj({ id: num })(JSON.parse(data), "新推送").id;

/** 推送渠道的显示名。 */
export const CHANNEL_LABELS: Record<string, string> = { web: "Web 通知", wechat: "微信" };
export const channelLabel = (c: string): string => CHANNEL_LABELS[c] ?? c;

/** 发往 Web 通知中心的才计未读、才提醒。 */
export const toWeb = (p: PushView): boolean => p.deliveries.some((d) => d.channel === "web");
