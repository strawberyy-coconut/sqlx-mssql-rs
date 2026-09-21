-- Initial schema for the driver walkthrough.
CREATE TABLE users (
    id UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    description NVARCHAR(MAX) NULL,
    add_date DATETIMEOFFSET NOT NULL
);
