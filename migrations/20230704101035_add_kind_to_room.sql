create type class_type as enum('webinar', 'p2p', 'minigroup');
alter table room add kind class_type;
