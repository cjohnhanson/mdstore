# mdstore

Generic parser and serializer for YAML-frontmatter markdown documents.
The storage layer underneath tisket and zettel.

`Document<T>` holds typed frontmatter (any `Serialize + Deserialize`)
and a string body. `parse` extracts the YAML between `---` fences and
returns the rest as body. `serialize` reconstructs canonical format.

Any tool that stores structured data as markdown files in git can use
this instead of hand-rolling frontmatter parsing. Also provides slug
generation, prefix handling, and document selection utilities.
