-- Add migration script here
ALTER TABLE event ALTER COLUMN original_created_by SET NOT NULL;
