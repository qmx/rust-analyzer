//! This module provides a way to construct a `File`.
//! It is intended to be completely decoupled from the
//! parser, so as to allow to evolve the tree representation
//! and the parser algorithm independently.
//!
//! The `TreeSink` trait is the bridge between the parser and the
//! tree builder: the parser produces a stream of events like
//! `start node`, `finish node`, and `FileBuilder` converts
//! this stream to a real tree.
use std::mem;

use crate::{
    SyntaxKind::{self, *},
    parsing::{ParseError, TreeSink},
};

/// `Parser` produces a flat list of `Event`s.
/// They are converted to a tree-structure in
/// a separate pass, via `TreeBuilder`.
#[derive(Debug)]
pub(crate) enum Event {
    /// This event signifies the start of the node.
    /// It should be either abandoned (in which case the
    /// `kind` is `TOMBSTONE`, and the event is ignored),
    /// or completed via a `Finish` event.
    ///
    /// All tokens between a `Start` and a `Finish` would
    /// become the children of the respective node.
    ///
    /// For left-recursive syntactic constructs, the parser produces
    /// a child node before it sees a parent. `forward_parent`
    /// saves the position of current event's parent.
    ///
    /// Consider this path
    ///
    /// foo::bar
    ///
    /// The events for it would look like this:
    ///
    ///
    /// START(PATH) IDENT('foo') FINISH START(PATH) COLONCOLON IDENT('bar') FINISH
    ///       |                          /\
    ///       |                          |
    ///       +------forward-parent------+
    ///
    /// And the tree would look like this
    ///
    ///    +--PATH---------+
    ///    |   |           |
    ///    |   |           |
    ///    |  '::'       'bar'
    ///    |
    ///   PATH
    ///    |
    ///   'foo'
    ///
    /// See also `CompletedMarker::precede`.
    Start {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },

    /// Complete the previous `Start` event
    Finish,

    /// Produce a single leaf-element.
    /// `n_raw_tokens` is used to glue complex contextual tokens.
    /// For example, lexer tokenizes `>>` as `>`, `>`, and
    /// `n_raw_tokens = 2` is used to produced a single `>>`.
    Token {
        kind: SyntaxKind,
        n_raw_tokens: u8,
    },

    Error {
        msg: ParseError,
    },
}

impl Event {
    pub(crate) fn tombstone() -> Self {
        Event::Start { kind: TOMBSTONE, forward_parent: None }
    }
}

/// Generate the syntax tree with the control of events.
pub(super) fn process(sink: &mut impl TreeSink, mut events: Vec<Event>) {
    let mut forward_parents = Vec::new();

    for i in 0..events.len() {
        match mem::replace(&mut events[i], Event::tombstone()) {
            Event::Start { kind: TOMBSTONE, .. } => (),

            Event::Start { kind, forward_parent } => {
                // For events[A, B, C], B is A's forward_parent, C is B's forward_parent,
                // in the normal control flow, the parent-child relation: `A -> B -> C`,
                // while with the magic forward_parent, it writes: `C <- B <- A`.

                // append `A` into parents.
                forward_parents.push(kind);
                let mut idx = i;
                let mut fp = forward_parent;
                while let Some(fwd) = fp {
                    idx += fwd as usize;
                    // append `A`'s forward_parent `B`
                    fp = match mem::replace(&mut events[idx], Event::tombstone()) {
                        Event::Start { kind, forward_parent } => {
                            forward_parents.push(kind);
                            forward_parent
                        }
                        _ => unreachable!(),
                    };
                    // append `B`'s forward_parent `C` in the next stage.
                }

                for (j, kind) in forward_parents.drain(..).rev().enumerate() {
                    let is_root_node = i == 0 && j == 0;
                    sink.start_branch(kind, is_root_node);
                }
            }
            Event::Finish => sink.finish_branch(i == events.len() - 1),
            Event::Token { kind, n_raw_tokens } => {
                sink.leaf(kind, n_raw_tokens);
            }
            Event::Error { msg } => sink.error(msg),
        }
    }
}
