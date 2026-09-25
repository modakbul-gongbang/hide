/** An area's state line and its actions: nothing open, opening, waiting or unavailable (S6 B9; S7 B10, B16). */
export function AreaEmpty({ state, text, children }: { state: string; text: string; children?: React.ReactNode }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-sm bg-background p-lg text-center text-caption text-muted-foreground" data-area-empty={state}>
      <p className="max-w-full break-words">{text}</p>
      {children ? <div className="flex flex-wrap justify-center gap-sm">{children}</div> : null}
    </div>
  );
}
