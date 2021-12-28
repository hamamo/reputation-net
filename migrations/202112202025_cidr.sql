alter table statement add cidr_min text;
alter table statement add cidr_max text;
create index idx_cidr_min on statement(cidr_min);
create index idx_cidr_max on statement(cidr_max);
/* some more indexes for other operations */
create index idx_entity_1 on statement(entity_1);
create index idx_opinion_fk_statement on opinion(statement_id);

