create view event_state as (
  select distinct
    first_value(id) over (
      partition by room_id, set, label
      order by occured_at DESC, created_at DESC
    ) AS id
  from event
);

create index event_state_idx
on event (room_id, set, label, occured_at, created_at)
where deleted_at is null;
