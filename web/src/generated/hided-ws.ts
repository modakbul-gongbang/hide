/* Generated from contracts/hided-ws.schema.json. */

/**
 * Handshake, server frames, close reason codes, and client dispatch events for hided.
 */
export type HidedWebSocketContract = Handshake | ServerFrame | ClientEvent;

export interface Handshake {
  token: string;
  schema_version: 2;
  have_revision?: number;
  have_terminal_sequence?: number;
}
export interface ServerFrame {
  type: "snapshot" | "delta" | "error";
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
