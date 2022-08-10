# event.create

Create an [event](../event.md#event) in a [room](../room.md#room).

The _room_ must be opened.

## Authorization

The tenant authorizes the current _agent_ for `create` action on
`["classrooms", classroom_id, "events", type, "authors", current_account_id]`.

In case `is_claim` parameter is `true` the object is
`["classrooms", classroom_id, "claims", type, "authors", current_account_id]`.

## Multicast request

Name          | Type    | Default    | Description
------------- | ------- | ---------- | -----------------------------
room_id       | uuid    | _required_ | The room's identifier.
type          | string  | _required_ | The event type.
set           | string  |       type | Collection set's name.
label         | string  | _optional_ | Collection item's label.
attribute     | string  | _optional_ | An attribute for authorization and filtering.
data          | json    | _required_ | The event JSON payload.
is_claim      | boolean |      false | Whether to notify the tenant.
is_persistent | boolean |       true | Whether to persist the event.
removed       | boolean |      false | Whether to "remove"[^1] the event


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


If `is_claim` is true a notification will be sent to the tenant

**URI:** `audiences/:room_audience/events`

**Label:** `event.create`.

**Payload:** [event](../event.md#event) object + optional `classroom_id`:

Name                 | Type     | Default    | Description
-------------------- | -------- | ---------- | -------------------------------------------------
id                   | uuid     | _required_ | The event's identifier.
room_id              | uuid     | _required_ | The room's identifier to which the event belongs.
type                 | string   | _required_ | The event type.
set                  | string   |       type | The set to which the event is related.
label                | string   | _optional_ | A label to identify an element within the set.
attribute            | string   | _optional_ | An attribute for authorization and filtering.
data                 | json     | _required_ | Schemaless payload of the event.
occurred_at          | int      | _required_ | Number of nanoseconds since the room's opening when the event took place.
original_occurred_at | int      | _required_ | `occurred_at` of the first event with the same `label`.
created_by           | agent_id | _required_ | An agent who created the event.
created_at           | int      | _required_ | The event's absolute creation timestamp in milliseconds.
classroom_id         | uuid     | _optional_ | If room belongs to a dispatcher's classroom - id of the classroom.

[^1]: All previous events with the same label will be excluded from set queries like they never existed.
