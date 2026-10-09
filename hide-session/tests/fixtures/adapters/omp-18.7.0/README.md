# omp 18.7.0 session contract

Synthetic conversation with no account history or credentials.
Native semantics come from can1357/oh-my-pi v18.7.0, commit e0fc1cf4ea354b445a359b37fa5eb58deaa85598: session-title-slot.ts, session-loader.ts, session-paths.ts, session-listing.ts and tools/ask.ts.
The physical first title slot is 256 UTF-8 bytes including newline.
It overrides the old header and title_change audit; its clear or source change must also revoke a previously observed title.
The following version3 session header owns identity and cwd, while parentSession is opaque fork lineage.
Native ask toolCall arguments carry questions and choices; toolResult correlates by toolCallId, including error results.
Metadata, developer and child artifact records do not become root human turns.
