ALTER TABLE event ADD COLUMN attribute TEXT;
CREATE INDEX event_attribute_idx ON event (attribute) WHERE deleted_at IS NULL;
