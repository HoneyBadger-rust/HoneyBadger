#![feature(test)]

extern crate test;
extern crate ratel;
extern crate toolshed;
extern crate serde_json;
extern crate ressa;

use test::Bencher;

static SOURCE: &'static str = r#"

'use strict';

/**
 * Extract red color out of a color integer:
 *
 * 0x00DEAD -> 0x00
 *
 * @param  {Number} color
 * @return {Number}
 */
function red( color )
{
    let foo = 3.14;
    return color >> 16;
}

/**
 * Extract green out of a color integer:
 *
 * 0x00DEAD -> 0xDE
 *
 * @param  {Number} color
 * @return {Number}
 */
function green( color )
{
    return ( color >> 8 ) & 0xFF;
}


/**
 * Extract blue color out of a color integer:
 *
 * 0x00DEAD -> 0xAD
 *
 * @param  {Number} color
 * @return {Number}
 */
function blue( color )
{
    return color & 0xFF;
}


/**
 * Converts an integer containing a color such as 0x00DEAD to a hex
 * string, such as '#00DEAD';
 *
 * @param  {Number} int
 * @return {String}
 */
function intToHex( int )
{
    const mask = '#000000';

    const hex = int.toString( 16 );

    return mask.substring( 0, 7 - hex.length ) + hex;
}


/**
 * Converts a hex string containing a color such as '#00DEAD' to
 * an integer, such as 0x00DEAD;
 *
 * @param  {Number} num
 * @return {String}
 */
function hexToInt( hex )
{
    return parseInt( hex.substring( 1 ), 16 );
}

module.exports = {
    red,
    green,
    blue,
    intToHex,
    hexToInt,
};

"#;

#[bench]
fn parse_to_ast(b: &mut Bencher) {
    b.bytes = SOURCE.len() as u64;

    b.iter(|| {
        let _module = ratel::parse(SOURCE).expect("Must parse");
    });
}

#[bench]
fn parse_to_ast_ressa(b: &mut Bencher) {
    b.bytes = SOURCE.len() as u64;

    b.iter(|| {
        let mut parser = ressa::Parser::new(SOURCE).expect("Failed to create parser");
        let _ = parser.parse().expect("Unable to parse text");
    })
}

#[bench]
fn tokenize(b: &mut Bencher) {
    let arena = toolshed::Arena::new();
    let ptr = arena.alloc_str_with_nul(SOURCE);
    b.bytes = SOURCE.len() as u64;

    b.iter(|| {
        let mut lexer = unsafe { ratel::lexer::Lexer::from_ptr(ptr) };

        while lexer.token != ratel::lexer::Token::EndOfProgram {
            lexer.consume()
        }
    });
}

#[bench]
fn serialize_to_json(b: &mut Bencher) {
    let module = ratel::parse(SOURCE).expect("Must parse");
    let output = serde_json::to_string(&module).unwrap();

    b.bytes = output.len() as u64;

    b.iter(|| {
        serde_json::to_string(&module).unwrap()
    })
}
