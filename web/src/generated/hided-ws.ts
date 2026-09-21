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
 * snapshot/delta carry the core state; error answers a malformed client event; directory_list answers a `remote_file_list` for the `local` target and path_refused answers a `remote_file_list` or `create_workspace` whose path failed the $HOME boundary (see directoryList and pathRefused).
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
