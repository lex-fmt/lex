### Fixed — annotation parameters keep their comma separator on format ([#703](https://github.com/lex-fmt/lex/issues/703))

The Lex serializer joined annotation parameters with a space instead of a comma, so `:: warning type=critical, id=123 ::` re-serialized as `:: warning type=critical id=123 ::` and re-parsed to a single parameter. Parameters now re-emit comma-separated.
