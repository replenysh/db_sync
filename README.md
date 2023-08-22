# DB Sync

This is a tool used to sync Postgres [functions](https://www.postgresql.org/docs/15/sql-createfunction.html) and [row level security (RLS) policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html) to a database based on definitions in a file tree.
It is valuable to manage these entities in a declarative way and there are numerous benefits, such as:

1. Not needing to copy/paste an entire function/policy to make a small change
2. See how a function changes over time with version control
3. Easily see what new changes are introduced in PR review

However, there are some limitations to `db_sync`, so please read this document carefully to understand the semantics.

## Install & use

`db_sync` can be installed as a nix flake. [Check it out on FlakesHub](https://flakehub.com/flake/jaredramirez/db_sync)

Here's output of running `db_sync --help`:

```
Program to sync function and RLS defintions defined declaratively to a database

Usage: db_sync [OPTIONS] <DB_URL>

Arguments:
  <DB_URL>

Options:
      --config-path <CONFIG_PATH>  [default: ./db_sync.toml]
  -h, --help                       Print help
  -V, --version                    Print version
```

`db_sync` only supports PostgresQL.

## Overview

`db_sync` takes a directory structure and syncs its definitions to a database. Given a file tree like:

```
functions/
├── schema_a
│   ├── types.sql
│   └── function_1.sql
└── schema_b
    └── function_2.sql

rls_policies/
├── schema_c
│   └── table_1.sql
└── schema_d
    └── table_2.sql
```

And a config file like:
```toml
[functions]
dir = './functions'
schemas = ['a', 'b']

[rls_policies]
dir = './rls_policies'
schemas = ['c', 'd']
```

Running `db_sync` will, in order:

1. Drop all RLS policies for all tables in schemas `c`, and `d`
2. Drop all functions and types in  schemas `a`, and `b` (will `DROP ... CASCADE` these functions/types)
3. Run all files named `types.sql` in `functions/`
4. Run all other files in `functions/`
5. Run all files `rls_policies/`

All steps are run in the same postgres transaction, so if anything fails all changes are rolledback and the database is untouched.

**Be really careful about what schemas use with `db_sync`** as it will drop all RLS policies in `rls_policies.schemas` and **`DROP CASCADE` all functions and types `functions.schemas`.** I recommend using a dedicated `functions` schema that's created specifically for this tool, that way `db_sync` doesn't accidentally drop something you didn't intend.

### Config

`db_sync` needs a config file. This config specifies the directories to use for functions & RLS policies.
This config  specifies the schemas to work with for both functions and policies as well as the directories to use.

Specifiying the schemas is an important detail!
If you tried to define `CREATE FUNCTION schema_d.new_function` in the file `functions/` dir
you'd get an error because `schema_d` isn't in `config.functions.schemas`. 

#### Shared types via `types.sql`

You can define files called `types.sql` in your `functions/` directory. These will run before the rest of the files, so it's a great place to  create shared types that the multiple functions use.

> Other than the `types.sql` files, there is no gaurenteed of order the rest of the functions files will be run. If you have multiple `types.sql` files in different directories, the order that the `types` files will be run in is also not gaurenteed. However, all `*/types.sql` files will be run before any function defintion file.

#### Functions bodies

Before the functions are run, we the command `SET check_function_bodies = FALSE;`. This disables the validation of bodies of functions, allowing you to reference other functions in other files without us having to do tons of cyclical dependency work. However, this means that some errors will not be caught until the functions are run. So always test you code! And write [pgtap](https://pgtap.org/) tests too!

### File structure

Beyond the root directory, the way the files are organizaed don't actually matter! You could have a file tree like:
```
functions/
├── schema_a_function_1.sql
└── schema_b_function_2.sql
```

So long as the files only use the schemas defined in `config.functions.schemas`, `db_sync` will work.

That said, we recommend using the 1st filetree structure mentioned in this document.

### Statements in the files

All statements across all files should only be `CREATE` statements, because we drop all existing functions/types/policies before running the definitions. 

In `functions/`, only the following statements are allow:
- `CREATE FUNCTION`
- `CREATE TYPE`
- `CREATE DOMAIN`
- `CREATE ENUM`
- `CREATE RANGE`

In `rls_policies/`, only the following statements are allow:
- `CREATE POLICY`

`db_sync` will error if there is an unallowed statement. 

## Roadmap

- Add `--dry-run` flag?
- ???
