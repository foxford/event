create view event_state as (
  select distinct
    first_value(id) over (
      partition by room_id, set, label
      order by occurred_at desc, created_at desc
    ) as id
  from event
);

create index event_state_idx
on event (room_id, set, label, occurred_at, created_at)
where deleted_at is null;
