# event.list

List [events](../event.md#event) in a [room](../room#room).

## Authorization

The tenant authorizes the current _agent_ for `list` action on `["rooms", room_id]` object.

## Multicast request

Name            | Type   | Default    | Description
--------------- | ------ | ---------- | ------------------
room_id         | string | _required_ | The room's identifier.
type            | string | _optional_ | The event's type filter.
set             | string | _optional_ | Collection set's filter.
label           | string | _optional_ | Collection item's filter.
last_created_at | int    | _optional_ | `created_at` value of the last seen event on the previous page in milliseconds.
direction       | string |    forward | Pagination direction: forward | backward.
limit           | int    |        100 | Limits the number of events in the response.

## Unicast response

**Status:** 200.

**Payload:** list of [events](../event.md#event).
