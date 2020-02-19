# state.read

Read [state](../state.md#state) of _sets_ in a [room](../room.md#room).

## Authorization

The tenant authorizes the current _agent_ for `list` action on
`["rooms", room_id, "events"]` object.

## Multicast request

Name            | Type     | Default    | Description
--------------- | -------- | ---------- | -------------------------------------------------------
room_id         | string   | _required_ | The room identifier.
sets            | [string] | _required_ | Set names to calculate the state for. 1 to 10 elements.
occured_at      | int      | _required_ | Number of milliseconds since room opening to specify the moment to calculate the state for.
last_created_at | int      | _optional_ | `created_at` value of the last seen event for pagination in milliseconds.
direction       | string   |    forward | Pagination direction: forward | backward.
limit           | int      |        100 | Limits the number of events in the response.

## Unicast response

**Status:** 200.

**Payload:** [state](../state.md#state) object. If `sets` parameter has only one element there will
also be `has_next` key with boolean value indicating that there's more data left for pagination
when `true`.
