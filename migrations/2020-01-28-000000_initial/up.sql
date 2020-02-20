create extension if not exists "pgcrypto";

create type account_id as (
    label text,
    audience text
);

create type agent_id as (
    account_id account_id,
    label text
);

create table room (
    id uuid default gen_random_uuid(),

    audience text not null,
    source_room_id uuid null,
    time tstzrange not null,
    tags json null,
    created_at timestamptz not null default now(),

    primary key (id),
    foreign key (source_room_id) references room (id) on delete set null
);

create table adjustment (
    room_id uuid,

    started_at timestamptz not null,
    segments int8range[] not null,
    occurred_at bigint not null,
    created_at timestamptz not null default now(),

    primary key (room_id),
    foreign key (room_id) references room (id) on delete cascade
);
