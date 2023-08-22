# DB Sync

This is a tool used to sync functions and RLS policies to a database based on definitions in a file tree.
Is is valuable to manage these entities in a declarative way and take advantage of version control in peer reviews.
There are some limitations to this tool, so please read this document carefully to understand the semantics.

## Usage

This tool can be used as a nix flake. [Check it out on FlakesHub](https://flakehub.com/flake/jaredramirez/db_sync)

## Overview

This tool takes a directory structure and syncs its definitions to a database. Given a file tree like:

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
```json
{
  "functions": {
    "dir": "./functions",
    "schemas": ["schema_a", "schema_b"]
  },
  "rls_policies": {
    "dir": "./rls_policies",
    "schemas": ["schema_c", "schema_d"]
  }
}

```

Running this tool will, in order:

1. Drop all RLS policies for all tables in schemas `c`, and `d`
2. Drop all functions and types in  schemas `a`, and `b` (will `DROP ... CASCADE` these functions/types)
3. Run all files named `types.sql` in `functions/`
4. Run all other files in `functions/`
5. Run all policy statements in `rls_policies/`

All steps are run in the same postgres transaction, so if anything fails all changes are rolledback and the database is untouched.

**Be really careful about what schemas you list here, this tool will drop all RLS policies in `rls_policies.schemas` and `DROP CASCADE` all functions and types `functions.schemas`.**

### Config

This tool needs a config file, defined here as `config.json`.

This config specifies the directories to use for functions & RLS policies. In the documentation, we'll assume these are defined as `functions/` and `rls_policies/` respectively.
Additionally, it specifies the schemas to work with for both functions and policies. This is an important detail!
If you tried to define `CREATE FUNCTION schema_d.new_function` in the file `functions/schema_a/new_function`.
This tool will error, because `schema_d` isn't in `config.functions.schemas`. 

### Filetree

The file tree doesn't actually matter! You could, in theory have a file tree like:
```
functions/
├── schema_a_function_1.sql
└── schema_b_function_2.sql
```

So long as the files only use the schemas defined in `config.functions.schemas`, this tool will work.

That said, we recommend using the 1st filetree structure mentioned in this document as it's the most organized and works nicely with git diffs. 

#### `types.sql`

You can define files called `types.sql` in your `functions/` directory. These will run before the rest of the files, so it's a great place to 
create types that the multiple other functions use.

> Other than the `types.sql` files, there is no gaurenteed order the rest of the functions files will be run. If you have multiple `types.sql` files in different directories, the order that the `types` files will be run in is also not gaurenteed. However, all `types` files will be run before any othe file.

#### Functions bodies

Before the functions are run, we the command `SET check_function_bodies = FALSE;`. This disables the validation of bodies of functions, allowing you to reference other functions in other files without us having to do tons of cyclical dependency work. However, this means that some errors will not be caught until the functions are run. So always test you code! And write pgtap tests too!

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

This tool will error if there is an unallowed statement. 
