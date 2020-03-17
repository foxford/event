-- Add `original_occurred_at` field.
ALTER TABLE event ADD COLUMN original_occurred_at BIGINT NULL;

-- Fill with `occurred_at` of the earliest event for each `label`.
UPDATE event AS e
SET original_occurred_at = oe.occurred_at
FROM (
    SELECT
        room_id,
        set,
        label,
        MIN(occurred_at) AS occurred_at
    FROM event
    WHERE deleted_at IS NULL
    GROUP BY room_id, set, label
) AS oe
WHERE e.room_id = oe.room_id
AND   e.set = oe.set
AND   e.label = oe.label;

UPDATE event
SET original_occurred_at = occurred_at
WHERE original_occurred_at IS NULL;

-- Don't allow NULL values from now on.
ALTER TABLE event ALTER COLUMN original_occurred_at SET NOT NULL;

-- Create trigger to automatically set `original_occurred_at` on insert.
CREATE OR REPLACE FUNCTION on_event_insert() RETURNS trigger AS $$
DECLARE
    original_occurred_at BIGINT;
BEGIN
    original_occurred_at := (
        SELECT occurred_at
        FROM event
        WHERE deleted_at IS NULL
        AND   room_id = NEW.room_id
        AND   set = NEW.set
        AND   label = NEW.label
        ORDER BY occurred_at
        LIMIT 1
    );

    NEW.original_occurred_at := COALESCE(original_occurred_at, NEW.occurred_at);
    RETURN NEW;
END;
$$ LANGUAGE 'plpgsql';

CREATE TRIGGER event_insert_trigger
    BEFORE INSERT
    ON event
    FOR EACH ROW
    EXECUTE PROCEDURE on_event_insert();

-- Drop old indexes.
DROP INDEX event_room_id_idx;
DROP INDEX event_occurred_at_created_at_idx;
DROP INDEX event_state_idx;

-- Create new state index.
CREATE INDEX event_state_idx
ON event (room_id, set, label, original_occurred_at, occurred_at)
WHERE deleted_at IS NULL;
