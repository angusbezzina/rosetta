# Virgil

This file is generated from `.virgil/glossary.json` by `task glossary:render`
and is not edited by hand.

Virgil is a chief of staff for your context: it captures what matters where it
happens, keeps it private in spheres, and briefs every person and agent who
needs it. This glossary is the one place each term is defined; code, tests, docs
and copy use these words.

An `_Avoid_` line lists words used for the term that are not its name.
`rule.glossary-terms` flags them in shown copy; a phrase in quotes after
`except` is another product's own name and is allowed.

## Product

**Sphere**:
A bounded world of context (a client, the house move, a side project) with its
own memory, view and rules. The core primitive and its settled public name.

**Capture**:
Context taken in the place it happens (a page in the browser, a message in
Slack, a decision in a meeting), with its source attached.

**Brief**:
The curated context of a sphere handed to a person or an agent, scoped and
permissioned.

**Group**:
The people who share one Virgil: its members, channels, tasks and the spheres
shared with it.
_Avoid_: workspace (except another product's own: "Linear workspace", "Slack workspace", "Discord workspace", "Notion workspace", "Google Workspace", "GitHub workspace", "Figma workspace", "bd workspace")

**Lesson**:
A dated observation an agent records about the codebase, anchored to the source
it describes; not an instruction until verified against that source.

**Memory**:
A durable claim Recall holds, with the evidence it stands on; it reaches only a
caller who can see every one of its sources.

**Task**:
A unit of tracked work in Virgil's own task list.
_Avoid_: bead (except another product's own: "import-beads")

**Member**:
A principal who belongs to a group, holding a role on the group's role ladder.

**Principal**:
An actor a group knows: a person, a persona members speak as, an agent such as a
bot, or a service.

**Persona**:
A principal that is a name members or Virgil speak as rather than an account of
its own; Virgil answers as one persona per sphere.

**Companion**:
An agent that takes part in a conversation as a participant. Virgil's own
companion answers as its persona in a companion channel or a group room.

**Conversation**:
The messages a channel or a thread holds, read in order; a conversation with
Virgil is ordinary `message` records like any other.

**Profile**:
The card other members see about a person: their picture, name, role, local time
and what they wrote about themselves. It opens as the Profile rail beside a
conversation and is set under Settings → Account → Personal info.

**Personal overview**:
What Virgil knows about a person in a group: their stated preferences, the
responsibilities assigned to them, and inferences labelled as such. They read
it, correct a line or pin one with `virgil overview`.

**Online status**:
Whether a member shows as online: a heartbeat heard from them within the last 70
seconds, which their desktop daemon sends each of their groups every 30 seconds,
reads as online unless they chose Away.

**Goal**:
The objective a body of work serves, written only by a person; one goal can span
spheres and repositories, and an epic names the goal it is for.

**Epic**:
A body of work handed over whole: a native Virgil epic, a bd epic or a Linear
project carried in from its tracker. Epics hold tasks and nest inside other
epics.

**Output**:
What a body of work actually produced (a commit, a pull request, an email, a
document, a post or a release), linked to the epic and the task it came out of.

**Question**:
Something Virgil needs a person to answer. One addressed to a member puts a
needs-you item in their inbox until it is answered or dismissed.

**Inbox**:
Each member's list of what they are told about: a mention or direct message, a
reply in a thread they follow, a task handed to them or a question put to them.
Its read state is shared across their devices.

**Approval**:
A person's decision to let one effect happen, such as sending mail, scoped and
expiring so that no agent holds a blanket yes.

**Prep**:
The briefing Virgil writes before a meeting in three sections (background,
talking points and actions) from the event and what Recall knows of its
attendees.

**Summary**:
What Virgil writes into a huddle's channel after it ends: a summary members may
edit and the actions it proposes. One participant's daemon is elected to write
it.
_Avoid_: recap

**Branding**:
A group's visual identity (its colours, neutral palette, radius, font and logo),
set by an admin, that the group's views and pages render with unless a page pins
its own.

## Surfaces

**Convos**:
The app's space for talking: channels, direct messages and companion threads.

**Channel**:
A place records are read in, of one kind: a chat channel, a direct message, a
feed, a companion room, the Virgil DM, a docket or a saved view. A sphere's own
store and a person's synced recall are channels too, never shown in the rail.

**Thread**:
A root record and the replies that name it as their parent. A reply to a
message, a comment on a task and a companion session are all threads.

**View**:
A saved query with a rendering: one HTML document, run in a sandboxed frame over
the query's rows, shown as a channel's tab or on its own in the rail.

**Page**:
A view of kind `page`: an editable document with its own revisions and comments,
made from a template.

**Document**:
Anything the Documents space lists: a live document, a page, a clip, link or
file handed over, a note or a transcript, and saved messages when asked for.

**Live document**:
A document authored in Virgil that members edit together, each revision sealed
by the device that wrote it; a page's body is one.

**Letterhead**:
The trusted part every view document carries ahead of the view's own region:
Virgil's tokens, reset, primitives and component runtime.

**Card**:
A bounded surface that presents one thing, drawn with the `Card` primitive.

**Huddle**:
A live audio and video room in a channel, shown in its timeline as one card that
keeps the transcript, summary and actions after it ends.

**Call**:
A huddle whose other participant is Virgil.

**HUD**:
The Electron app's always-on-top window over the desktop: the flame, its pill
and the menu-bar item, present from login and outliving every product window.

**Overlay**:
Virgil's own fields on a task or epic carried from a tracker, which a person
edits in Virgil and which never leave it.

**Clipper**:
The browser extension that clips a page or a selection into a sphere's recall,
talking only to the daemon on loopback.

**Space**:
One of the app's top-level places: Convos, Activity, Meetings and Documents in
the far-left rail, and Settings from the account menu.

**Activity**:
What happened to you, newest first: each thread you follow with its newest reply
and unread count, and each reaction to your messages. It is a space in the app
and a tab of the HUD's pill, where inbox items join it.

**Home**:
The app's space, still behind a flag, that shows a person's feed with the views
they keep on their dashboard above it.

**Feed**:
What happened in the sources a person follows, such as GitHub and Linear,
reflected into Virgil and ranked with a reason; the Home space, still behind a
flag, shows it.

**Docket**:
A channel that holds the comments on tasks: the group's Tasks docket, and one
per connected Linear or bd workspace. Each comment is a message parented to its
task.

**Comment**:
A reply anchored to a task or a page. On a task it is a message in a docket
parented to the task; on a page it is a thread reply pinned to a quoted passage
of one revision, open until addressed or resolved.

**Attachment**:
A file a message carries or a task holds, never both; a task's file may also be
shown inline in its description.

**Block**:
One unit of a message's or a live document's body (a paragraph, heading, list,
quote, code, table or embedded view), made of runs of text, links, mentions and
emoji.

**Template**:
Reusable page source in the group's library: a layout, content slots, authoring
instructions, resources and how it takes the group's branding. A new page is
made from one and keeps its own copy.

**Flame**:
Virgil's mark and its default presence, whose colour says its state: blue while
Virgil is present, gold while it works, green when something landed and orange
when somebody needs you.

**Presence**:
The character Virgil appears as in the HUD and the app: the flame or an authored
animated avatar chosen at setup, which plays a clip for what Virgil is doing.

**Pill**:
The HUD's panel beside the flame: a fixed command bar over one board, which
shows a meeting being recorded or an opened prep, and otherwise the chosen tab
of Today, Tasks, Virgil and Activity.

## Architecture

**Door**:
The `virgil-serve` service that answers a request; it never streams.

**Edge**:
`apps/realtime-edge`, the only live transport.

**Worker**:
The `virgil-serve` service that runs the outbox, events and lifecycle work.

**Delivery**:
The `virgil-serve` service that sends through providers.

**Daemon**:
`virgil-desktop`, the always-on process where product logic lives; the Electron
app is its thin front end.

**Records**:
The typed contract for the group's data, generated from one source in
`packages/virgil-schema` into Rust, JSON Schema and TypeScript.

**Recall**:
Virgil's memory, run in the daemon and reached through the map, find, fetch,
related and prime verbs and the personal overview.

**Authority**:
The authority that holds a container's records: it accepts each write, gives it
its place in the container's one order and stamps its time.

**Cell**:
One deployment of the door, worker and delivery services with its own signing
identity, serving the authorities placed on it.

**Portal**:
`apps/portal`, the account web service: it signs people and their devices in and
knows which groups each account belongs to.

**Device**:
A signing identity bound to a principal: the key one machine or agent signs its
writes with, which the door verifies.

**Replica**:
A device's local copy of the records it may read, kept current through the door
and the edge, which the daemon answers reads from.

**Snapshot**:
A signed copy of the current records a device may read in a container at one
point in its order, which refills a replica without replaying history.

**Outbox**:
The committed operations the door writes, one per log entry in the same
transaction, for the worker to publish to the edge.

**Bundle**:
The packaged desktop application a release ships, which the daemon verifies,
swaps in whole and keeps one of for rollback.

**ACP**:
The Agent Client Protocol, which an AI provider speaks on its standard input and
output; every generative session the daemon opens runs over it.

**MCP**:
The Model Context Protocol, through which Virgil hands its own tools to each ACP
session as one server on loopback HTTP.

**Witness**:
The native inference runtime the daemon loads for speech recognition and Recall
embeddings.

**Container**:
The unit records are stored, ordered and shared in, served by one authority. A
group's root container holds its public channels; each direct message, private
channel and sphere gets its own, and each person has a private one.

**Command**:
A registered action with a dotted id such as `message.send`. Buttons, shortcuts,
the palette, Virgil and the MCP tools change anything only by invoking one.

**AI provider**:
A program that speaks ACP and that Virgil's own work runs on, such as the
bundled Codex or Claude, chosen and signed in under Settings. No model key
passes through Virgil.
_Avoid_: LLM profile

## Agents

**Cato**:
The merge agent: it readies a draft pull request, verifies it merged onto main
and moves main to that commit.

**Messo**:
The maintenance agent: it works the findings of the rulebook's sweep and
reported bugs into draft pull requests, and never merges.

**Rulebook**:
`whetstone/rulebook.json`: the mission, the principles and the rules Virgil's
gates and Messo's sweep enforce.

**Session**:
One ACP job an AI provider runs for Virgil. A companion session is projected
into records as a thread of its companion channel.

## Development

**Party**:
An isolated local stack of Virgil's services with one app per test person,
signed into a shared group, that `task app:dev` starts.

**Lane**:
One CI job, selected by the paths a change touches, that runs a set of checks on
its own runner and that `ci-gate` waits on.

**Gate**:
A check that must pass before work moves on, such as `ci-gate` before a pull
request lands. Its green proves only what passed under the conditions it ran
with.

**Skill**:
A packaged set of instructions, a `SKILL.md`, that teaches an agent one kind of
task.

**Atlas**:
The typed model of a repository's parts, how they connect and the views drawn
from it, kept in `.virgil/atlas.json` beside the glossary.

**Design token**:
A named design value (a colour, font, radius, shadow, duration or spring) from
`packages/virgil-tokens`, the one place a Virgil surface takes its values from.

**Whetstone**:
The rule checker that runs the rulebook against a pull request's change or, in
sweep mode, the whole tree, and reports each finding under its rule. An outside
tool Virgil pins by version.

**PRTL**:
The outside tool that runs coding agents on the owner's machines, its nodes, in
isolated worktrees and turns their work into verified commits. Virgil tasks say
what to do; PRTL runs it.
