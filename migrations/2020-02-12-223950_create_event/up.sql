create table event (
    id uuid default gen_random_uuid(),
    room_id uuid not null,
    kind text not null,
    set text not null,
    label text,
    data jsonb not null default '{}'::jsonb,
    occured_at bigint not null,
    created_by agent_id not null,
    created_at timestamptz not null default now(),
    deleted_at timestamptz,

    foreign key (room_id) references room (id) on delete cascade,
    primary key (id)
);

create index event_room_id_idx on event (room_id) where deleted_at is null;
create index event_kind_idx on event (kind) where deleted_at is null;
create index event_set_label_idx on event (set, label) where deleted_at is null;
create index event_occured_at_created_at_idx on event (occured_at, created_at) where deleted_at is null;
