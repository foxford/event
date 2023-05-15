# event.list

List [events](../event.md#event) in a [room](../room#room).

## Authorization

The tenant authorizes the current _agent_ for `read` action on `["classrooms", classroom_id]` object.

## Multicast request

Name             | Type               | Default    | Description
---------------- | ------------------ | ---------- | ------------------
room_id          | string             | _required_ | The room's identifier.
type             | string or [string] | _optional_ | The event's type filter. Works like IN for arrays.
set              | string             | _optional_ | Collection set's filter.
label            | string             | _optional_ | Collection item's filter.
attribute        | string             | _optional_ | Attribute filter.
last_occurred_at | int                | _optional_ | `occurred_at` value of the last seen event on the previous page in nanoseconds.
direction        | string             |    forward | Pagination direction: forward | backward.
limit            | int                |       100к | Limits the number of events in the response.

## Unicast response

**Status:** 200.

**Payload:** list of [events](../event.md#event).
