create table event (
    id uuid default gen_random_uuid(),
    room_id uuid not null,
    kind text not null,
    set text not null,
    label text,
    data jsonb not null default '{}'::jsonb,
    "offset" bigint not null,
    created_by agent_id not null,
    created_at timestamptz not null default now(),
    deleted_at timestamptz,

    foreign key (room_id) references room (id) on delete cascade,
    primary key (id)
);
