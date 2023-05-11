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
