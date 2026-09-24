// Small line marks drawn in the current text colour, so every state colour
// comes from a token class on the element that holds them.

export function LayoutIcon({ mode }: { mode: "agents" | "together" | "views" }) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.25">
      {mode === "agents" ? (
        <>
          <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
          <path d="M4.5 6.5l2 1.5-2 1.5M8 10h3" />
        </>
      ) : mode === "together" ? (
        <>
          <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
          <path d="M8 2.5v11M3.5 6.5l1.5 1-1.5 1M10 6h3M10 8h3M10 10h2" />
        </>
      ) : (
        <>
          <path d="M4 1.5h5.5l3 3v10H4z" />
          <path d="M9.5 1.5v3h3M6 8h4.5M6 10h4.5M6 12h3" />
        </>
      )}
    </svg>
  );
}

export function ToolIcon({ tool }: { tool: "explorer" | "changes" }) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.25">
      {tool === "explorer" ? (
        <path d="M1.5 3.5h4.5l1.5 1.5h7v8.5h-13z" />
      ) : (
        <>
          <circle cx="4.5" cy="4" r="1.75" />
          <circle cx="4.5" cy="12" r="1.75" />
          <circle cx="11.5" cy="8" r="1.75" />
          <path d="M4.5 5.75v4.5M6.25 12c3 0 5.25-1.5 5.25-2.25" />
        </>
      )}
    </svg>
  );
}
