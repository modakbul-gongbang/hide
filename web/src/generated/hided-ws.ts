/* Generated from contracts/hided-ws.schema.json. */

/**
 * Handshake, server frames, close reason codes, path refusal reason codes, and client dispatch events for hided.
 */
export type HidedWebSocketContract =
  Handshake | ServerFrame | ClientEvent | WorkspaceQuery | WorkspaceAction | WorkspaceResult;
export type WorkspaceRequestId = string;
export type WorkspaceCommand =
  | {
      action: "open_file";
      path: string;
      beside: boolean;
      reveal: boolean;
    }
  | {
      action: "open_diff";
      path: string;
      beside: boolean;
      reveal: boolean;
    }
  | {
      action: "open_browser";
      url: string;
      reveal: boolean;
    }
  | {
      action: "select";
      view_id: string;
      reveal: boolean;
    }
  | {
      action: "close";
      view_id: string;
    }
  | {
      action: "split";
      view_id: string;
      area_id: string;
      edge: "left" | "right" | "up" | "down";
    }
  | {
      action: "move";
      view_id: string;
      area_id: string;
      index: number;
    };

export interface Handshake {
  token: string;
  schema_version: 2;
  client_kind?: "web" | "desktop";
  have_revision?: number;
  have_terminal_sequence?: number;
}
/**
 * snapshot/delta carry the core state; error answers a malformed client event; directory_list answers a `file_list`, and path_refused answers any event whose path failed its boundary - the $HOME line for `create_workspace`, the checkout-root line for `file_list`, `file_open`, `reveal_path`, `file_save`, `file_create`, `dir_create`, `path_rename`, `path_move`, `path_trash` and `file_bytes` (see directoryList and pathRefused). A `file_list`, `file_open`, `reveal_path`, `file_save`, `file_create`, `dir_create`, `path_rename`, `path_move` or `path_trash` that names an SSH device in `device_id` never meets this machine's checkout roots: the core accepts it only for the checkout in front on that device and the device's helper confines every path to that checkout's opened root. file_bytes_error answers a `file_bytes` read the daemon could not serve as bytes (see fileBytes). directory_unavailable answers a `file_list` for a checkout on an SSH device that its helper could not list (see directoryUnavailable). directory_changed announces that a watched folder changed, so the client re-reads that one folder (see directoryChanged). attachment_refused answers an `attachment_stage` or `attachment_commit` that could not be staged (see attachments). file_index_result answers a `file_index` query with the ranked checkout paths (see fileIndex). daemon is the first frame after a valid handshake and describes the daemon itself (see daemonInfo). A `file_bytes` read that succeeds is answered with one or more binary frames on the same socket, not a text frame (see fileBytes).
 */
export interface ServerFrame {
  type:
    | "daemon"
    | "snapshot"
    | "delta"
    | "error"
    | "directory_list"
    | "directory_unavailable"
    | "path_refused"
    | "file_bytes_error"
    | "directory_changed"
    | "file_index_result"
    | "attachment_refused"
    | "open_external_result";
  payload: {
    [k: string]: unknown;
  };
  message?: string;
}
export interface ClientEvent {
  schema_version: 2;
  kind: string;
  payload: {
    [k: string]: unknown;
  };
  [k: string]: unknown;
}
/**
 * A pane-scoped query on a capability-authenticated WebSocket. The daemon derives device, pane, and Workspace from the credential and current core projection.
 */
export interface WorkspaceQuery {
  type: "workspace_query";
  request_id: WorkspaceRequestId;
  query: "info" | "view_list";
}
/**
 * One pane-scoped document or View transition. Repeating the same request ID and command returns its recorded result within the ten-minute retry window.
 */
export interface WorkspaceAction {
  type: "workspace_action";
  request_id: WorkspaceRequestId;
  command: WorkspaceCommand;
}
/**
 * A pane-scoped request outcome. A successful action result includes context, request_id, changed, view_id, and optionally area_id; a refusal names reason and next_action.
 */
export interface WorkspaceResult {
  type: "workspace_result";
  request_id?: WorkspaceRequestId;
  ok: boolean;
  result?: {
    [k: string]: unknown;
  };
  reason?: string;
  next_action?: string;
  [k: string]: unknown;
}
/**
 * One row of a snapshot's `navigator.provider_usage`: a provider's seven-day account window as herdr-core/src/usage.rs reads it (docs/AI_PROVIDERS.md, Weekly usage display). state is `loading` until the first read answers, `available` for a current read, `stale` for a success kept while the provider is offline (message says how old), `fallback` for Codex's latest local session window, and `unavailable` with message saying why, a window whose reset has passed included. used_percent and resets_at_unix_seconds are null in `loading` and `unavailable`. buckets are scoped windows under the row, such as one model's own weekly limit.
 */
export interface ProviderUsage {
  provider: string;
  label: string;
  window_minutes: number;
  state: "loading" | "available" | "stale" | "fallback" | "unavailable";
  used_percent: number | null;
  resets_at_unix_seconds: number | null;
  message: string | null;
  last_checked_at_unix_ms: number | null;
  last_success_at_unix_ms: number | null;
  last_error_kind: string | null;
  buckets: ProviderUsageBucket[];
}
/**
 * A scoped window under a providerUsage row. state is the row's own `available` or `stale` while the bucket has a value, and `unavailable` with message when its line could not be read or its reset has passed.
 */
export interface ProviderUsageBucket {
  label: string;
  state: "available" | "stale" | "unavailable";
  used_percent: number | null;
  resets_at_unix_seconds: number | null;
  message: string | null;
}
/**
 * Two fields a `ui_state_update` payload may carry beside the UI state it replaces; the core never persists or echoes them, and an absent field keeps the core's value. usage_window_visible: a shell window is on screen, so the core reads each provider every five minutes. usage_popover_open: the Weekly Usage popover is open; its change from false to true reads again any provider last read more than a minute ago.
 */
export interface UiStateUsageHints {
  usage_window_visible?: boolean;
  usage_popover_open?: boolean;
  [k: string]: unknown;
}
/**
 * focus_checkout: one admitted focus and optional sidebar disclosure transition. Unknown workspace/checkout refuses the disclosure intent before either value changes.
 */
export interface FocusCheckoutPayload {
  workspace_id: string;
  checkout_id: string;
  focus_device?: boolean;
  display_id?: string;
  expanded?: boolean;
}
