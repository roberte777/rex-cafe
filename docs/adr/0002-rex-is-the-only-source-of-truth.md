# Rex is the Café's only source of truth

Everything the Café believes about an Agent comes from the Rex server: a snapshot at startup, then Rex events, chiefly Program Status (OSC 7501) records. It never reads Claude Code hooks or transcripts, scrapes `ps`, parses terminal titles to infer Status, or polls. Those sources are easy to reach, but each one ties the Café to one agent's internals. Agents that report Status to Rex appear with no extra work, and agents that don't simply aren't in the Café. A local clock drives animation only and never changes what the Café believes.
