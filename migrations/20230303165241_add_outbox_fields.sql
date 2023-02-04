alter table event
    add source_command_id uuid;

alter table event
    add entity_type text;

alter table event
    add entity_event_id bigint;

alter table event
    add constraint uniq_entity_type_entity_event_id
        unique (entity_type, entity_event_id);
