begin;

-- Connect to the legacy DB.
create extension if not exists "dblink";
create foreign data wrapper legacy_db_wrapper validator postgresql_fdw_validator;
create server legacy_db foreign data wrapper legacy_db_wrapper options (hostaddr '${SOURCE_HOST}', dbname '${SOURCE_DB}');
create user mapping for postgres server legacy_db options (user '${SOURCE_USER}', password '${SOURCE_PASSWORD}');
select dblink_connect('legacy_db');
grant usage on foreign server legacy_db to postgres;

-- Disable indexes.
update pg_index
set indisready = false
where indrelid in (
    select oid
    from pg_class
    where relname in ('room', 'adjustment', 'event')
);

-- Disable triggers (including foreign key checks).
alter table room disable trigger all;
alter table adjustment disable trigger all;
alter table event disable trigger all;

-- Migrate rooms.
insert into room (id, audience, source_room_id, time, created_at)
select
    id,
    audience,
    parent_id as source_room_id,
    tstzrange(opened_at, closed_at) as time,
    created_at
from dblink('legacy_db', '
    select
        id,
        created_at,
        opened_at,
        closed_at,
        audience,
        parent_id
    from rooms
    where deleted_at is null
') as data(
    id uuid,
    created_at timestamptz,
    opened_at timestamptz,
    closed_at timestamptz,
    audience varchar(1024),
    parent_id uuid
);

-- Migrate adjustments.
insert into adjustment (room_id, started_at, segments, "offset", created_at)
select
    id as room_id,
    opened_at as started_at, -- Legacy DB doesn't store it so we presume opened_at = started_at.
    array( -- jsonb [[1, 2], [3, 4]] -> int8range[] {[1, 2), [3, 4)}
        select int8range((segment->0)::bigint, (segment->1)::bigint, '[)')
        from jsonb_array_elements(stream->'fragments') as segment
    ) as segments,
    (stream->'preroll')::bigint as "offset",
    closed_at as created_at
from dblink('legacy_db', '
    select
        id,
        opened_at,
        closed_at,
        stream
    from rooms
    where deleted_at is null
    and   stream != ''{}''::jsonb
') as data(
    id uuid,
    opened_at timestamptz,
    closed_at timestamptz,
    stream jsonb
);

-- Migrate events.
insert into event (id, room_id, kind, set, label, data, occurred_at, created_by, created_at)
select
    id,
    room_id,
    type as kind,
    type as set,
    case type
        when 'unsubscribe' then data->'conn_id'
        when 'subscribe' then data->'conn_id'
        when 'document' then data->'url'
        when 'document-delete' then data->'url'
        when 'stream' then id::text
        when 'message' then id::text
        when 'draw' then id:text
        else null
    end as label,
    data,
    "offset" * 1000000 as occurred_at,
    ('(' || account_id || ',' || audience || ')', 'web')::agent_id as created_by,
    created_at
from dblink('legacy_db', '
    select
        id,
        type,
        room_id,
        created_at, 
        data,
        audience,
        account_id,
        "offset"
    from events
    where deleted_at is null    
') as data(
    id uuid,
    type varchar(255),
    room_id uuid,
    created_at timestamptz,
    data jsonb,
    audience varchar(1024),
    account_id varchar(1024),
    "offset" bigint
);

-- Enable triggers.
alter table room enable trigger all;
alter table adjustment enable trigger all;
alter table event enable trigger all;

-- Enable indexes.
update pg_index
set indisready = false
where indrelid in (
    select oid
    from pg_class
    where relname in ('room', 'adjustment', 'event')
);

-- Reindex tables.
reindex table room;
reindex table adjustment;
reindex table event;

-- Cleanup.
drop server legacy_db cascade;
drop foreign data wrapper legacy_db_wrapper;
drop extension "dblink";

commit;
