DROP INDEX event_state_idx;

CREATE INDEX event_state_idx
ON event (room_id, set, label, occurred_at, created_at)
WHERE deleted_at IS NULL;

CREATE INDEX event_occurred_at_created_at_idx
ON event (occurred_at, created_at)
WHERE deleted_at IS NULL;

CREATE INDEX event_room_id_idx
ON event (room_id)
WHERE deleted_at IS NULL;

DROP TRIGGER event_insert_trigger ON event;
DROP FUNCTION on_event_insert;
ALTER TABLE event DROP COLUMN original_occurred_at;
