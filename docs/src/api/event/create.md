# event.create

Create an [event](../event.md#event) in a [room](../room.md#room).

The _room_ must be opened.

## Authorization

The current _agent_ must be [entered](../room/enter.md) to the _room_.

The tenant authorizes the current _agent_ for `create` action on
`["rooms", room_id, "events"]` object.

## Multicast request

Name    | Type   | Default    | Description
------- | ------ | ---------- | -----------------------
room_id | uuid   | _required_ | The room identifier.
type    | string | _required_ | Event type.
set     | string |       type | Collection set name.
label   | string | _optional_ | Collection item label.
data    | json   | _required_ | The event JSON payload.

The _type_ and _data_ is arbitrary except
[stream editing events](../event.md#stream-editing-events).

_set_ and _label_ are also arbitrary but they impact [state](../state.md#state).
Check out [rules](../state.md#event-creation-from-the-state-perspective) on how to choose them.

## Unicast response

**Status:** 201.

**Payload:** [event](../event.md#event) object.

## Broadcast event

A notification is being sent to all [agents](../agent.md#agent) that have
[entered](../room/enter.md) the room.

**URI:** `rooms/:room_id/events`

**Label:** `event.create`.

**Payload:** [event](../event.md#event) object.
