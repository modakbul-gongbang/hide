// One screen request's life (PRD software-factory-ui B10): sending until the
// engine's answer arrives in the `factory` section under its request id,
// then taken or refused with the engine's next action. A request the core
// has not answered after a while reads as refused, never as sending forever,
// and still takes the engine's answer if it comes later: a slow `gh` call
// can outlast the wait while the engine finishes the work.

import { useEffect, useState } from "react";
import type { Actions } from "../actions";
import { useShellStore } from "../store";
import type { FactoryCommand } from "./commands";
import type { ActionAnswer } from "./model";

/** How long a request may go unanswered before the screen says it was not taken. */
export const REQUEST_ANSWER_TIMEOUT_MS = 20_000;

export type RequestState =
  | { phase: "idle" }
  | { phase: "sending"; id: string }
  | { phase: "taken"; answer: ActionAnswer["answer"] }
  | { phase: "refused"; answer: ActionAnswer["answer"] }
  | { phase: "refused"; answer: null; id: string };

export type FactoryRequest = { state: RequestState; send: (command: FactoryCommand) => void; reset: () => void };

export function useFactoryRequest(actions: Pick<Actions, "factoryAction">): FactoryRequest {
  const [state, setState] = useState<RequestState>({ phase: "idle" });
  const id = state.phase === "sending" || (state.phase === "refused" && state.answer === null) ? state.id : null;
  const answer = useShellStore((s) => (id ? (s.factory?.actions.find((row) => row.request_id === id)?.answer ?? null) : null));
  useEffect(() => {
    if (id === null || answer === null) return;
    setState(answer.ok ? { phase: "taken", answer } : { phase: "refused", answer });
  }, [id, answer]);
  useEffect(() => {
    if (id === null) return undefined;
    const timer = window.setTimeout(() => setState((current) => (current.phase === "sending" && current.id === id ? { phase: "refused", answer: null, id } : current)), REQUEST_ANSWER_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [id]);
  return {
    state,
    send: (command) => setState({ phase: "sending", id: actions.factoryAction(command) }),
    reset: () => setState({ phase: "idle" }),
  };
}
