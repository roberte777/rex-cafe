# Rex Café

A pixel-art café that shows every agent on a Rex server as a customer, and lets you reach those agents by interacting with the scene.

## Language

**Café**:
The whole scene: room, furniture, Barista, and one Clawd per Agent.
_Avoid_: Dashboard, view, scene (when meaning the app)

**Agent**:
A coding-agent program (Claude Code, Codex, OpenCode, Pi) in a Rex pane that reports its Status to Rex. Other programs that report Status, such as build tools, are not Agents.
_Avoid_: Session, bot, worker

**Clawd**:
The pixel-art customer that represents exactly one Agent in the Café.
_Avoid_: Avatar, sprite, character, crab

**Hat**:
The headwear and matching colour a Clawd keeps for as long as its Agent is in the Café.
_Avoid_: Colour, badge, tag

**Barista**:
The café's staff character, standing in for Rex itself; present only while the Café is connected to Rex.
_Avoid_: Rex (when meaning the character), server

**Closed**:
The Café while it has no connection to Rex: no Barista, and a sign on the door.
_Avoid_: Offline, disconnected

**Status**:
What an Agent has reported to Rex about itself: idle, working, blocked, done, or error.
_Avoid_: State, activity

**Activity**:
What a Clawd is doing on screen, choreographed from changes in its Agent's Status.
_Avoid_: Status, state, animation

**Legend**:
The strip below the room listing each Agent's name, Hat colour, and current Activity.
_Avoid_: Sidebar, status bar

**Menu**:
The chalkboard tally of Agents by Status, written as drinks.
_Avoid_: Chalkboard, counts, stats

### Activities

**Walking in**:
A Clawd entering through the door because its Agent has just appeared.

**Coffee break**:
A seated Clawd whose Agent is idle.

**Getting coffee**:
A Clawd fetching a drink from the Barista as its Agent starts working.

**Working**:
A seated Clawd whose Agent is working.

**Needs you**:
A Clawd whose Agent is blocked on the user: a permission, a question, or a login.
_Avoid_: Waiting, attention

**Done**:
A Clawd whose Agent has finished work the user hasn't looked at yet.

**Spilled**:
A Clawd whose Agent has failed and stopped.
_Avoid_: Crashed, failed

**Leaving**:
A Clawd walking out of the door because its Agent is gone.

### Places

**Seat**:
One of the twelve table places a Clawd can occupy.

**Standing spot**:
A place at the counter for a Clawd when every Seat is taken.

**Outside**:
Where an Agent is when the room is full: listed in the Legend, but with no Clawd in the room.
