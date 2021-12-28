create table if not exists statement(
    id integer primary key,
    name text not null,
    entity_1 text not null,
    entity_2 text,
    entity_3 text,
    entity_4 text,
    cidr_min text,
    cidr_max text,
    unique(name,entity_1,entity_2,entity_3,entity_4)
);