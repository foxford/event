# room.enter

Enter a [room](api.room.md#room).

The room must be opened.

After entrance the current _agent_ starts to receive room notifications including real-time
[events](api.event.md#room) and is able to call endpoints that require _room_ entrance.

Entered [agents](api.agent.md#agent) appear in the active _agents_ [list](api.agent.list.md).

## Authorization

The tenant authorizes the current _agent_ for `subscribe` action on
`["rooms", room_id, "events"]` object.

## Paramteres

Name | Type | Default    | Description
---- | ---- | ---------- | --------------------
id   | uuid | _required_ | The room identifier.

## Response

**Status:** 202.

**Payload:** empty object.

Receiving the response doesn't yet mean that the agent has already entered the room but that
the process has been initiated. Before making any requests that require room entrance one must wait
for the `room.enter` broadcast notification that confirms the entrance. The description is below.

## Notification

A notification is being sent to all [agents](api.agent.md#agent) that have
[entered](api.room.enter.md) the room.

**URI:** `rooms/:room_id/events`

**Label:** `room.enter`

**Payload:**

Name     | Type     | Default    | Description
-------- | -------- | ---------- | --------------------
id       | uuid     | _required_ | The room identifier.
agent_id | agent_id | _required_ | The agent identifier.
