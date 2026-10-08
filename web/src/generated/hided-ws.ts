/* Generated from contracts/hided-ws.schema.json. */

/**
 * Handshake, server frames, close reason codes, path refusal reason codes, and client dispatch events for hided.
 */
export type HidedWebSocketContract =
  Handshake | ServerFrame | ClientEvent | WorkspaceQuery | WorkspaceAction | LinksQuery | WorkspaceResult;
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
  /**
   * The `terminal_sequence` of the last terminal frame a reconnecting client applied. With a delta it resumes every pane from there (a pane the hub's ring no longer reaches is drawn again from a full frame); a client that kept its terminals and names none has every pane drawn again; a client given a whole snapshot ignores it.
   */
  have_terminal_sequence?: number;
}
/**
 * snapshot/delta carry the core state; terminal carries pane output beside them, never through the core (see terminalFrame); error answers a malformed client event; directory_list answers a `file_list`, and path_refused answers any event whose path failed its boundary - the $HOME line for `create_workspace` and `clone_repository`, the checkout-root line for `file_list`, `file_open`, `reveal_path`, `file_save`, `file_create`, `dir_create`, `path_rename`, `path_move`, `path_trash` and `file_bytes` (see directoryList and pathRefused). A `file_list`, `file_open`, `reveal_path`, `file_save`, `file_create`, `dir_create`, `path_rename`, `path_move` or `path_trash` that names an SSH device in `device_id` never meets this machine's checkout roots: the core accepts it only for the checkout in front on that device and the device's helper confines every path to that checkout's opened root. file_bytes_error answers a `file_bytes` read the daemon could not serve as bytes (see fileBytes). directory_unavailable answers a `file_list` for a checkout on an SSH device that its helper could not list (see directoryUnavailable). directory_changed announces that a watched folder changed, so the client re-reads that one folder (see directoryChanged). attachment_refused answers an `attachment_stage` or `attachment_commit` that could not be staged (see attachments). file_index_result answers a `file_index` query with the ranked checkout paths (see fileIndex). project_target answers a `project_target` probe (see projectTarget and projectTargetResult). clone_target answers a `clone_target` question about where a clone would land (see cloneTarget). daemon is the first frame after a valid handshake and describes the daemon itself (see daemonInfo). A `file_bytes` read that succeeds is answered with one or more binary frames on the same socket, not a text frame (see fileBytes).
 */
export interface ServerFrame {
  type:
    | "daemon"
    | "snapshot"
    | "delta"
    | "terminal"
    | "error"
    | "directory_list"
    | "directory_unavailable"
    | "path_refused"
    | "project_target"
    | "file_bytes_error"
    | "directory_changed"
    | "file_index_result"
    | "attachment_refused"
    | "open_external_result"
    | "clone_target";
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
 * `hide links`: a read of the link record on a capability-authenticated WebSocket, answered as a workspace_result without a shell. The caller checkout's Project is the scope unless all_projects is set; a target only another Project holds is refused with other_project, and one the record never saw with not_found.
 */
export interface LinksQuery {
  type: "links";
  request_id: WorkspaceRequestId;
  query: {
    all_projects?: boolean;
    target:
      | {
          kind: "pr";
          number: number;
        }
      | {
          kind: "issue";
          number: number;
        }
      | {
          kind: "branch";
          name: string;
        }
      | {
          kind: "session";
          id: string;
        };
  };
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
  project_expanded?: boolean;
}
/**
 * The core's shared explicit interface language. Null follows each client's primary system language, with English for unsupported languages. Invalid stored values publish en with a diagnostic and remain stored until an explicit edit.
 */
export type InterfaceLanguage = null | "en" | "ko" | "zh-CN" | "ja";

/**
 * Payload of interface_language_set. This event alone edits the core preference; ui_state_update never changes it.
 */
export interface InterfaceLanguageSetPayload {
  language: InterfaceLanguage;
}
/**
 * Payload of a terminal frame: pane output from the screen-side hub (PRD core-host-node-terminal D-11, D-22), in the order the panes' nodes produced it. One sequence numbers every pane's chunks; terminal_sequence is the client's cursor after this frame, which it names as have_terminal_sequence when it reconnects. After a whole snapshot the client resets every terminal and is sent a terminal frame with no chunks that gives its new cursor. A pane whose output a client left unsent past 1 MiB, or that a resume cannot reach, sends that client nothing more until a full frame redraws it. Keys go the other way as the `key` event {pane_id or pending_request, bytes_base64}, and a view's grid as `terminal_viewport` {pane_id, cols, rows, new_view}; both reach the pane's node directly and never the core.
 */
export interface TerminalFrame {
  chunks: {
    pane_id: string;
    bytes_base64: string;
  }[];
  terminal_sequence: number;
}
