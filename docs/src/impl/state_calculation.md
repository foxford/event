# State calculation

When calling [state.read](../api/state/read.md) method, the endpoint makes a DB query per each
item of _sets'_ parameters. These parameters are limited to 10 elements.

A query filters _events_ by _room_ and _set_ and also filters by `occurred_at` relative
timestamp and absolute `created_at` greater (or less in case of backwards direction) than the
corresponding parameters.

Then it inner joins the query with the `event_state` view that returns only actual events by
applying a `FIRST_VALUE` window function partitioning by a label.

Then it comes to double ordering by `occurred_at` and `created_at` fields. Ordering just by
created_at is not sufficient because the room may be edited. Ordering just by `occurred_at`
is not sufficient because [adjustment](room_adjustment.md) operation may collapse numerous events
(more than the pagination `limit`) into a single `occurred_at` value.
