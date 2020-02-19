# event.create

Create an [event](../event.md#event) in a [room](../room.md#room).

The _room_ must be opened.

## Authorization

The current _agent_ must be [entered](../room/enter.md) to the _room_.

The tenant authorizes the current _agent_ for `create` action on
`["rooms", room_id, "events"]` object.

## Parameters

Name    | Type   | Default    | Description
------- | ------ | ---------- | -----------------------
room_id | uuid   | _required_ | The room identifier.
type    | string | _required_ | Event type.
set     | string |       type | Collection set name.
label   | string | _optional_ | Collection item label.
data    | json   | _required_ | The event JSON payload.

## Response

**Status:** 201.

**Payload:** [event](../event.md#event) object.

## Notification

A notification is being sent to all [agents](../agent.md#agent) that have
[entered](../room/enter.md) the room.

**URI:** `rooms/:room_id/events`

**Label:** `event.create`.

**Payload:** [event](../event.md#event) object.
