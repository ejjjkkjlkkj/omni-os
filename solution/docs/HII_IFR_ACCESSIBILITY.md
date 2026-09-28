# HII/IFR accessibility bridge

UEFI HII forms are already semantic data. The bridge consumes the same IFR opcodes used by a Forms Browser instead of scraping pixels.

Implemented mappings include:
- FORM -> form
- SUBTITLE/TEXT -> text
- ONE_OF -> radio group
- CHECKBOX -> checkbox
- NUMERIC -> spin button
- PASSWORD -> password edit
- ACTION/RESET_BUTTON -> button
- REF -> link
- DATE/TIME/STRING -> edit
- ORDERED_LIST -> list

The parser fails closed on:
- zero/short opcode lengths
- records crossing buffer bounds
- scope underflow
- unclosed scopes
- malformed HII package lengths
- question records too short to contain an EFI_IFR_QUESTION_HEADER

It extracts Prompt ID, Help ID and Question ID from standard statement/question headers. String package resolution is deliberately separate so bounds validation stays independent.

A PASSWORD opcode is tagged as secret-bearing metadata. Secret values are never extracted by this parser.
