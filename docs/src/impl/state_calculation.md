# State calculation

When calling [state.read](../api/state/read.md) method the endpoint makes a DB query per each
item of _sets_ parameter. This parameter is limited to 10 elements.

A query filters _events_ by _room_ and _set_ and also filters the select by `occured_at` relative
timestamp and absolute `created_at` greater (or less in case of backwards direction) than the
corresponding paramters.

Then it inner joins the query with the `event_state` view that returns only actual events by
applying a `FIRST_VALUE` window function partitioning by label.

Then it comes to double ordering by `occured_at` and `created_at` fields. Just ordering by
created_at is not sufficient because the room may be edited and ordering just by `occured_at`
is not sufficient because [adjustment](room_adjustment.md) operation may collapse many events
(more than the pagination `limit`) into a single `occured_at` value.
