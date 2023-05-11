-- IMPORTANT NOTE:
-- These commands must be performed on the PostgreSQL server BEFORE running this migration.
-- Reason: SQLx wraps migrations in transactions in which we cannot create indexes concurrently.
-- 
-- alter table event
--     add if not exists entity_type text;
-- 
-- alter table event
--     add if not exists entity_event_id bigint;
-- 
-- create unique index concurrently if not exists event_entity_type_entity_event_id_idx on event (entity_type, entity_event_id);

alter table event
    add if not exists source_command_id uuid;

alter table event
    add if not exists entity_type text;

alter table event
    add if not exists entity_event_id bigint;

DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_constraint 
                   WHERE conname = 'uniq_entity_type_entity_event_id') THEN 
            alter table event
                add constraint uniq_entity_type_entity_event_id
                unique (entity_type, entity_event_id);
    END IF;
END $$;
