//! Lexes arbitrary UTF-8 and checks that the tokens reproduce the input.

#![no_main]
#![forbid(unsafe_code)]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|source: &str| {
    let lexed = stav::lex(source);
    let mut offset = 0;
    for token in &lexed.tokens {
        assert_eq!(token.span.start, offset);
        offset = token.span.end;
    }
    let text: String = lexed.tokens.iter().map(|t| t.text(source)).collect();
    assert_eq!(text, source);
});
