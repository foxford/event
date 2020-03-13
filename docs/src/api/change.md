# Change

A _change_ is a set of changes that corresponds to an event update, event addition or removal

Change is of _type_ `addition`, `modification` or `removal` which dictates the properties required.

- `addition`
    required properties: `event_type`, `event_set`, `event_label`, `event_data`, `event_occurred_at` and `event_created_by`

- `modification`
    required properties: `event_id`
    everything else event related is optional

- `removal`
    required properties: `event_id`

## Properties

Name                | Type          | Default    | Description
------------------- | ------------- | ---------- | -------------------------------------------------
id                  | uuid          | _required_ | The change's identifier.
edition_id          | uuid          | _required_ | The edition this change belongs to.
type                | string        | _required_ | Change type, possible values are 'addition', 'modification', 'removal'.
event_id            | uuid          | _optional_ | The event this change applies to.
event_type          | string        | _optional_ | New value for the event kind.
event_set           | string        |       type | New value for the event set.
event_label         | string        | _optional_ | New value for the event label.
event_data          | json          | _optional_ | New value for the event data.
event_occurred_at   | int           | _optional_ | New value for the event occurred_at.
event_created_by    | agent_id      | _optional_ | New value for the event created_by.
created_at          | int           | _optional_ | The change's absolute creation timestamp in seconds.
