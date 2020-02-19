# event.list

List [events](api.event.md#event) in a [room](api.room#room).

## Authorization

The tenant authorizes the current _agent_ for `list` action on
`["rooms", room_id, "events"]` object.

## Parameters

Name            | Type   | Default    | Description
--------------- | ------ | ---------- | ------------------
room_id         | string | _required_ | The room identifier.
type            | string | _optional_ | Event type filter.
set             | string | _optional_ | Collection set filter.
label           | string | _optional_ | Collection item filter.
last_created_at | int    | _optional_ | `created_at` value of the last seen event on the previous page in milliseconds.
direction       | string |    forward | Pagination direction: forward | backward.
limit           | int    |        100 | Limits the number of events in the response.

## Response

**Status:** 200.

**Payload:** list of [events](api.event.md#event).
