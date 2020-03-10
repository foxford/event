# event.create

Create an [event](../event.md#event) in a [room](../room.md#room).

The _room_ must be opened.

## Authorization

The current _agent_ must [be in](../room/enter.md) the _room_.

The tenant authorizes the current _agent_ for `create` action on
`["rooms", room_id, "events", type, "authors", current_account_id]`.

In case `is_claim` parameter is `true` the object is
`["rooms", room_id, "claims", type, "authors", current_account_id]`.

## Multicast request

Name          | Type    | Default    | Description
------------- | ------- | ---------- | -----------------------------
room_id       | uuid    | _required_ | The room's identifier.
type          | string  | _required_ | The event type.
set           | string  |       type | Collection set's name.
label         | string  | _optional_ | Collection item's label.
data          | json    | _required_ | The event JSON payload.
is_claim      | boolean |      false | Whether to notify the tenant.
is_persistent | boolean |       true | Whether to persist the event.

The _type_ and _data_ is arbitrary except
[stream editing events](../event.md#stream-editing-events).

The _set_ and _label_ are also arbitrary, but they impact a [state](../state.md#state).
Check out [rules](../state.md#event-creation-from-the-state-perspective) on how to choose them.

## Unicast response

**Status:** 201.

**Payload:** [event](../event.md#event) object.

## Broadcast event

A notification is being sent to all [agents](../agent.md#agent) that
[are in](../room/enter.md) the room.

**URI:** `rooms/:room_id/events`

**Label:** `event.create`.

**Payload:** [event](../event.md#event) object.
