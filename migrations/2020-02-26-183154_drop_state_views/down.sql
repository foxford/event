create view event_state_backward as (
  select distinct
    first_value(id) over (
      partition by room_id, set, label
      order by occurred_at desc, created_at desc
    ) as id
  from event
);

create view event_state_forward as (
  select distinct
    first_value(id) over (
      partition by room_id, set, label
      order by occurred_at, created_at
    ) as id
  from event
);
