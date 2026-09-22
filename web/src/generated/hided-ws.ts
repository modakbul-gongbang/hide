/* Generated from contracts/hided-ws.schema.json. */

/**
 * Handshake, server frames, close reason codes, path refusal reason codes, and client dispatch events for hided.
 */
export type HidedWebSocketContract = Handshake | ServerFrame | ClientEvent;

export interface Handshake {
  token: string;
  schema_version: 2;
  have_revision?: number;
  have_terminal_sequence?: number;
}
/**
 * snapshot/delta carry the core state; error answers a malformed client event; directory_list answers a `remote_file_list` for the `local` target or a `file_list`, and path_refused answers any event whose path failed its boundary - the $HOME line for `remote_file_list` and `create_workspace`, the checkout-root line for `file_list`, `file_open`, `reveal_path`, `file_save`, `file_create`, `dir_create`, `path_rename`, `path_move` and `path_trash` (see directoryList and pathRefused).
 */
export interface ServerFrame {
  type: "snapshot" | "delta" | "error" | "directory_list" | "path_refused";
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
