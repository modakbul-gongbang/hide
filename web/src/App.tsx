import { useEffect, useRef } from "react";
import { ConnectionBadge } from "./badge";
import { Sidebar } from "./sidebar";
import { TerminalPane, feedChunks } from "./terminal";
import { connectShell, type DispatchFn } from "./ws";

const noop: DispatchFn = () => {};

export function App() {
  const dispatch = useRef<DispatchFn>(noop);
  useEffect(() => {
    const session = connectShell({ onChunks: feedChunks });
    dispatch.current = session.dispatch;
    return () => session.close();
  }, []);
  return (
    <div className="flex h-full flex-col bg-background text-primary">
      <ConnectionBadge />
      <div className="flex min-h-0 flex-1">
        <Sidebar dispatch={(event) => dispatch.current(event)} />
        <TerminalPane dispatch={(event) => dispatch.current(event)} />
      </div>
    </div>
  );
}
