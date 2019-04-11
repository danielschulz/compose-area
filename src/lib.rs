#![cfg_attr(test, feature(proc_macro_hygiene))]

#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::non_ascii_literal, clippy::single_match_else, clippy::if_not_else,
         clippy::similar_names)]

#[macro_use] extern crate log;

#[macro_use]
mod macros;

mod caret_pos;
mod extract;
mod utils;

use std::mem;

use cfg_if::cfg_if;
use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{self, Element, Node, CharacterData, Text, Range};

pub use crate::caret_pos::{
    CaretPosition,
    Position,
    get_caret_position,
    set_caret_position,
    unset_caret_position,
};
use crate::extract::extract_text;
use crate::utils::is_text_node;

cfg_if! {
    // When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
    // allocator.
    if #[cfg(feature = "wee_alloc")] {
        extern crate wee_alloc;
        #[global_allocator]
        static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
    }
}

/// The context object containing the state.
#[wasm_bindgen]
pub struct ComposeArea {
    window: web_sys::Window,
    document: web_sys::Document,
    wrapper_id: String,
    caret_start: u32,
    caret_end: u32,
}

/// The node at the current caret position.
#[derive(Debug, PartialEq)]
pub struct NodeIndexOffset {
    /// Node index.
    pub index: u32,
    /// Byte offset from the node start.
    pub offset: u32,
}

/// This enum is relevant when determining the current node while the caret is
/// exactly between two nodes.
///
/// Depending on this enum value, the node before or after the cursor is returned.
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Direction {
    Before,
    After,
}

/// Initialize a new compose area wrapper with the specified `id`.
#[wasm_bindgen]
pub fn bind_to(id: &str) -> ComposeArea {
    utils::set_panic_hook();
    utils::init_log();

    info!("Bind to #{}", id);

    let window = web_sys::window().expect("No global `window` exists");
    let document = window.document().expect("Should have a document on window");
    let wrapper: Element = document.get_element_by_id(id).expect("Did not find wrapper element");

    // Initialize the wrapper element
    let div = document.create_element("div").expect("Could not create div");
    div.set_attribute("id", id).expect("Could not set wrapper id");
    div.set_attribute("class", "cawrapper initialized").expect("Could not set wrapper class");
    div.set_attribute("contenteditable", "true").expect("Could not set contenteditable attr");
    let br = document.create_element("br").expect("Could not create br");
    div.append_child(&br).expect("Could not append br");
    wrapper.replace_with_with_node_1(&div).expect("Could not initialize wrapper");
    mem::forget(wrapper); // Has been replaced, dead DOM reference

    info!("Initialized #{}", id);

    ComposeArea {
        window: window,
        document: document,
        caret_start: 0,
        caret_end: 0,
        wrapper_id: id.to_owned(),
    }
}

trait SizedNode {
    fn html_size(&self) -> u32;
}

impl SizedNode for Node {
    fn html_size(&self) -> u32 {
        match self.node_type() {
            Node::TEXT_NODE => self.unchecked_ref::<CharacterData>().length(),
            Node::ELEMENT_NODE => {
                let element = self.unchecked_ref::<Element>();
                let html_size = element.outer_html().encode_utf16().count();
                make_u32!(html_size)
            },
            other => {
                warn!("Unhandled node type: {}", other);
                0
            },
        }
    }
}

#[wasm_bindgen]
impl ComposeArea {

    /// Return a reference to the wrapper element.
    fn get_wrapper(&self) -> Element {
        self.document.get_element_by_id(&self.wrapper_id).expect("Did not find wrapper element")
    }

    /// Show the current caret position.
    ///
    /// Note: This does not query the DOM!
    pub fn get_caret_position(&self) -> CaretPosition {
        CaretPosition {
            start: self.caret_start,
            end: self.caret_end,
            success: true,
        }
    }

    /// Update the caret position.
    ///
    /// Note: This does not query or update the DOM!
    pub fn set_caret_position(&mut self, start: u32, end: u32) {
        self.caret_start = start;
        self.caret_end = end;
    }

    /// Update the caret position from DOM.
    ///
    /// Read the actual position from the DOM using the selection API and then
    /// overwrite the caret position in the state object.
    ///
    /// If the caret position is not within the wrapper element, then the
    /// internal state is not changed.
    ///
    /// Call this after every action that might have modified the DOM.
    pub fn update_caret_position_from_dom(&mut self) {
        debug!("WASM: update_caret_position");

        // Refresh caret pos
        let wrapper = self.get_wrapper();
        let pos = get_caret_position(&wrapper);
        if pos.success {
            assert!(pos.start <= pos.end);
            self.caret_start = pos.start;
            self.caret_end = pos.end;
        }
    }


    /// Insert an image at the current caret position.
    pub fn insert_image(&mut self, src: &str, alt: &str, cls: &str) {
        debug!("WASM: insert_image ({})", &alt);

        let img = self.document.create_element("img").expect("Could not create img element");
        img.set_attribute("src", &src).expect("Could not set attribute");
        img.set_attribute("alt", &alt).expect("Could not set attribute");
        img.set_attribute("class", &cls).expect("Could not set attribute");

        self.insert_node(img.unchecked_into());

        self.set_dom_caret_position_from_state();
        self.normalize();
    }

    /// Insert plain text at the current caret position.
    pub fn insert_text(&mut self, text: &str) {
        debug!("WASM: insert_text ({})", &text);

        let text_node = self.document.create_text_node(text);

        self.insert_node(text_node.unchecked_into());

        self.set_dom_caret_position_from_state();
        self.normalize();
    }

    /// Increment the caret position by the HTML size of the specified node.
    fn increment_caret_pos(&mut self, node: &Node) {
        self.caret_start += node.html_size();
        self.caret_end = self.caret_start;
    }

    /// Normalize the contents of the wrapper element.
    ///
    /// See https://developer.mozilla.org/en-US/docs/Web/API/Node/normalize
    fn normalize(&self) {
        self.get_wrapper().normalize();
    }

    /// Return the last range of the selection (if any).
    fn get_range(&self) -> Option<Range> {
        let selection = match self.window.get_selection().expect("Could not get selection from window") {
            Some(sel) => sel,
            None => {
                error!("Could not find selection");
                return None;
            },
        };
        selection.get_range_at(selection.range_count() - 1).ok()
    }

    /// Insert the specified node at the current caret position and
    /// increment the internal caret position.
    ///
    /// Note: The caret position is *not* written to the DOM!
    fn insert_node(&mut self, node: Node) {
        debug!("WASM: insert_node");

        // Remove current selection
        self.remove_selection();

        // Get wrapper
        let wrapper = self.get_wrapper();

        // Find the current node we're at
        if let Some(current_node) = self.find_node_at(self.caret_start, Direction::After) {
            // Get reference node
            let reference_node = wrapper.child_nodes()
                .get(current_node.index)
                .expect("Reference node not found");
            let offset = current_node.offset;

            // If we're between two nodes, insert element there.
            if offset == 0 {
                wrapper.insert_before(&node, Some(&reference_node))
                    .expect("Could not insert element");
                self.increment_caret_pos(&node);
                return;
            }

            // Otherwise, if we're at a text node, split it and insert element in between.
            if is_text_node(&reference_node) {
                let text_node: Text = reference_node.unchecked_into();
                let split_node: Text = text_node.split_text(offset)
                    .expect("Could not split text");
                wrapper.insert_before(&node, Some(&split_node))
                    .expect("Could not insert element");
                self.increment_caret_pos(&node);
                return;
            }

            // If we're in the middle of a non-text node, append the element.
            wrapper.insert_before(&node, reference_node.next_sibling().as_ref())
                .expect("Could not insert element");
            self.increment_caret_pos(&node);
        }

        // If none was found, insert at end
        wrapper.append_child(&node)
            .expect("Could not append child");
        self.increment_caret_pos(&node);
    }

    /// Return the node at the current caret start position and the offset from
    /// the beginning of that node.
    ///
    /// If the cursor is exactly between two nodes, then either the following
    /// or the preceding node is returned, depending on the `direction` chosen.
    ///
    /// If the current caret position is after the end of the last node and the
    /// direction is `Before`, then the last node will be returned, with
    /// corrected offset.
    fn find_node_at(&self, mut offset: u32, direction: Direction) -> Option<NodeIndexOffset> {
        let mut html_size: u32 = 0;

        // Query nodes
        let wrapper = self.get_wrapper();
        let nodes = wrapper.child_nodes();
        let node_count = nodes.length();

        // If there are no nodes, we can return immediately
        if node_count == 0 {
            return None;
        }

        // Iterate through the nodes
        for index in 0..node_count {
            let node = nodes.get(index).expect("Node not found");

            // If we're exactly at the start of the node, we can stop looking further.
            if offset == 0 {
                return match direction {
                    Direction::Before if index == 0 => None,
                    Direction::Before => Some(NodeIndexOffset { offset: html_size, index: index - 1 }),
                    Direction::After => Some(NodeIndexOffset { offset, index }),
                };
            }

            // Calculate node size
            html_size = node.html_size();

            // Update offset
            match offset.checked_sub(html_size) {
                Some(new_offset) => {
                    // If we're at the end and the caller wanted the node before
                    // the current caret position, return the current node.
                    if new_offset == 0 && direction == Direction::Before {
                        return Some(NodeIndexOffset { offset, index });
                    }
                    offset = new_offset;
                },
                None => {
                    // Underflow. Once we're below 0, we found the node.
                    return Some(NodeIndexOffset { offset, index });
                },
            }
        }

        // We reached the end of the node list.
        match direction {
            Direction::Before => {
                assert!(offset > 0);
                // Fall back to the last node, but fix the offset.
                let last_node = nodes.get(node_count - 1).expect("Could not find last node");
                Some(NodeIndexOffset {
                    offset: last_node.html_size(),
                    index: node_count - 1,
                })
            }
            Direction::After => None,
        }
    }

    /// If a selection range is present in the wrapper, remove its contents,
    /// update the caret position and return `true`. Otherwise, return `false`.
    ///
    /// TODO: Make sure that selection is within the wrapper!
    pub fn remove_selection(&mut self) -> bool {
        // Get the current selection range
        let range = match self.get_range() {
            Some(range) => range,
            None => return false,
        };

        // If range is collapsed, nothing needs to be removoed
        if range.collapsed() {
            return false;
        }

        // Remove contents
        match range.delete_contents() {
            Ok(()) => {
                self.update_caret_position_from_dom();
                self.normalize();
                true
            },
            Err(_) => {
                error!("Could not delete range contents");
                false
            }
        }
    }

    /// Set the caret position in the DOM using the current state.
    fn set_dom_caret_position_from_state(&self) {
        // Query nodes
        let wrapper = self.get_wrapper();
        let nodes = wrapper.child_nodes();

        let start_opt = self.find_node_at(self.caret_start, Direction::After);
        let end_opt = if self.caret_end > self.caret_start {
            self.find_node_at(self.caret_end, Direction::Before)
        } else {
            None
        };

        match (start_opt, end_opt) {
            (Some(start), Some(end)) => {
                let start_node = nodes.get(start.index)
                    .expect(&format!("Node at index {} not found", start.index));
                let end_node = nodes.get(end.index)
                    .expect(&format!("Node at index {} not found", end.index));
                set_caret_position(
                    &Position::Offset(&start_node, start.offset),
                    Some(&Position::Offset(&end_node, end.offset)),
                );
            }
            (Some(start), None) => {
                let start_node = nodes.get(start.index)
                    .expect(&format!("Node at index {} not found", start.index));
                set_caret_position(&Position::Offset(&start_node, start.offset), None);
            }
            (None, _) => {
                // We're at the end of the node list.
                let index = nodes.length() - 1;
                match nodes.get(index) {
                    Some(ref node) => set_caret_position(&Position::After(&node), None),
                    None => unreachable!(format!("Node at index {} not found", index)),
                }
            }
        }
    }

    /// Extract the text in the compose area.
    ///
    /// Convert elements like images to alt text.
    pub fn get_text(&self, no_trim: bool) -> String {
        let wrapper = self.get_wrapper();
        extract_text(&wrapper, no_trim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use wasm_bindgen_test::*;
    use wbg_rand::{Rng, wasm_rng};

    wasm_bindgen_test_configure!(run_in_browser);

    fn init(empty: bool) -> ComposeArea {
        // Get references
        let window = web_sys::window().expect("No global `window` exists");
        let document = window.document().expect("Should have a document on window");

        // Create wrapper element
        let wrapper = document.create_element("div").expect("Could not create wrapper div");
        let id = format!(
            "wrapper-{}",
            wasm_rng()
                .gen_ascii_chars()
                .take(10)
                .collect::<String>()
        );
        wrapper.set_attribute("id", &id).unwrap();
        document.body().unwrap().append_child(&wrapper).unwrap();

        // Bind to wrapper
        let ca = bind_to(&id);

        // Make sure that no nodes are left
        if empty {
            let wrapper = ca.get_wrapper();
            while wrapper.has_child_nodes() {
                wrapper.remove_child(&wrapper.last_child().unwrap()).unwrap();
            }
        }

        ca
    }

    /// Create and return a text node.
    fn text_node(ca: &ComposeArea, text: &str) -> Node {
        ca.document.create_text_node(text).unchecked_into()
    }

    /// Create and return an image node.
    fn image_node(ca: &ComposeArea) -> Node {
        let img = ca.document.create_element("img").unwrap();
        img.set_attribute("src", "img.jpg").unwrap();
        img.set_attribute("alt", "😀").unwrap();
        img.set_attribute("class", "em").unwrap();
        img.unchecked_into()
    }

    /// Return the nth child node.
    fn nth_child(ca: &ComposeArea, n: u32) -> Node {
        ca.get_wrapper().child_nodes().get(n).unwrap()
    }

    #[derive(Copy, Clone, Debug)]
    struct Img {
        src: &'static str,
        alt: &'static str,
        cls: &'static str,
    }

    impl Img {
        fn html(&self) -> String {
            format!(r#"<img src="{}" alt="{}" class="{}">"#, self.src, self.alt, self.cls)
        }

        fn html_size(&self) -> u32 {
            self.html().encode_utf16().count() as u32
        }
    }

    mod find_node_at {
        use super::*;

        struct FindStartNodeTest {
            children: Vec<Node>,
            caret_pos: u32,
            before: Option<NodeIndexOffset>,
            after: Option<NodeIndexOffset>,
        }

        impl FindStartNodeTest {
            fn test(&self, ca: ComposeArea) {
                for child in self.children.iter() {
                    ca.get_wrapper().append_child(child).unwrap();
                }
                assert_eq!(ca.find_node_at(self.caret_pos, Direction::Before), self.before);
                assert_eq!(ca.find_node_at(self.caret_pos, Direction::After), self.after);
            }
        }

        /// Empty node list
        #[wasm_bindgen_test]
        fn empty() {
            FindStartNodeTest {
                children: vec![],
                caret_pos: 0,
                before: None,
                after: None,
            }.test(init(true));
        }

        /// Before the first node
        #[wasm_bindgen_test]
        fn before_first() {
            let ca = init(true);
            FindStartNodeTest {
                children: vec![
                    ca.document.create_text_node("ab").unchecked_into(),
                ],
                caret_pos: 0,
                before: None,
                after: Some(NodeIndexOffset { offset: 0, index: 0 }),
            }.test(ca);
        }

        /// In the middle of a text node
        #[wasm_bindgen_test]
        fn in_text() {
            let ca = init(true);
            FindStartNodeTest {
                children: vec![text_node(&ca, "ab")],
                caret_pos: 1,
                before: Some(NodeIndexOffset { offset: 1, index: 0 }),
                after: Some(NodeIndexOffset { offset: 1, index: 0 }),
            }.test(ca);
        }

        /// At the end
        #[wasm_bindgen_test]
        fn at_end() {
            let ca = init(true);
            FindStartNodeTest {
                children: vec![text_node(&ca, "ab")],
                caret_pos: 2,
                before: Some(NodeIndexOffset { offset: 2, index: 0 }),
                after: None,
            }.test(ca);
        }

        /// Between two nodes
        #[wasm_bindgen_test]
        fn between_two() {
            let ca = init(true);
            FindStartNodeTest {
                children: vec![
                    ca.document.create_text_node("ab").unchecked_into(),
                    ca.document.create_element("br").unwrap().unchecked_into(),
                ],
                caret_pos: 2,
                before: Some(NodeIndexOffset { index: 0, offset: 2 }),
                after: Some(NodeIndexOffset { index: 1, offset: 0 }),
            }.test(ca);
        }

        #[wasm_bindgen_test]
        fn outofbounds() {
            let ca = init(true);
            // Caret position cannot be negative, but it can be larger than the
            // total length. Set it to 1 position *after* the end.
            FindStartNodeTest {
                children: vec![text_node(&ca, "ab"), text_node(&ca, "cde")],
                caret_pos: 6,
                before: Some(NodeIndexOffset { index: 1, offset: 3 }),
                after: None,
            }.test(ca);
        }
    }

    mod remove_selection {
        use super::*;

        struct State {
            start: u32,
            end: u32,
            nodes: u32,
        }

        struct RemoveSelectionTest {
            children: Vec<Node>,
            before: State,
            after: State,
            removed: bool,
        }

        impl RemoveSelectionTest {
            fn test(&self, ca: &mut ComposeArea) {
                for child in self.children.iter() {
                    ca.get_wrapper().append_child(child).unwrap();
                }
                ca.set_caret_position(self.before.start, self.before.end);
                ca.set_dom_caret_position_from_state();

                assert_eq!(ca.caret_start, self.before.start);
                assert_eq!(ca.caret_end, self.before.end);
                assert_eq!(ca.get_wrapper().child_nodes().length(), self.before.nodes);

                assert_eq!(ca.remove_selection(), self.removed);

                assert_eq!(ca.caret_start, self.after.start);
                assert_eq!(ca.caret_end, self.after.end);
                assert_eq!(ca.get_wrapper().child_nodes().length(), self.after.nodes);
            }
        }

        #[wasm_bindgen_test]
        fn remove_nothing() {
            let mut ca = init(true);
            RemoveSelectionTest {
                children: vec![text_node(&ca, "ab")],
                before: State { start: 1, end: 1, nodes: 1 },
                after: State { start: 1, end: 1, nodes: 1 },
                removed: false,
            }.test(&mut ca);
            assert_eq!(
                ca.get_wrapper().child_nodes().get(0).unwrap().text_content().unwrap(),
                "ab",
            );
        }

        #[wasm_bindgen_test]
        fn remove_entire_text_node() {
            let mut ca = init(true);
            RemoveSelectionTest {
                children: vec![text_node(&ca, "ab")],
                before: State { start: 0, end: 2, nodes: 1 },
                after: State { start: 0, end: 0, nodes: 0 },
                removed: true,
            }.test(&mut ca);
        }

        #[wasm_bindgen_test]
        fn remove_partial_text_node_middle() {
            let mut ca = init(true);
            RemoveSelectionTest {
                children: vec![text_node(&ca, "abcde")],
                before: State { start: 1, end: 3, nodes: 1 },
                after: State { start: 1, end: 1, nodes: 1 },
                removed: true,
            }.test(&mut ca);
            assert_eq!(nth_child(&ca, 0).text_content().unwrap(), "ade");
        }

        #[wasm_bindgen_test]
        fn remove_partial_text_node_to_end() {
            let mut ca = init(true);
            RemoveSelectionTest {
                children: vec![text_node(&ca, "abcde"), text_node(&ca, "f")],
                before: State { start: 1, end: 5, nodes: 2 },
                after: State { start: 1, end: 1, nodes: 1 },
                removed: true,
            }.test(&mut ca);
            assert_eq!(nth_child(&ca, 0).text_content().unwrap(), "af");
        }

        #[wasm_bindgen_test]
        fn remove_partial_text_node_past_end() {
            let mut ca = init(true);
            RemoveSelectionTest {
                children: vec![text_node(&ca, "abcde"), text_node(&ca, "fgh")],
                before: State { start: 1, end: 7, nodes: 2 },
                after: State { start: 1, end: 1, nodes: 1 },
                removed: true,
            }.test(&mut ca);
            assert_eq!(nth_child(&ca, 0).text_content().unwrap(), "ah");
        }

        #[wasm_bindgen_test]
        fn remove_entire_image_node() {
            let mut ca = init(true);
            RemoveSelectionTest {
                children: vec![text_node(&ca, "a"), image_node(&ca), text_node(&ca, "b")],
                before: State { start: 1, end: 1 + image_node(&ca).html_size(), nodes: 3 },
                after: State { start: 1, end: 1, nodes: 1 },
                removed: true,
            }.test(&mut ca);
            assert_eq!(nth_child(&ca, 0).text_content().unwrap(), "ab");
        }

        /// If the caret end is shorter than the node, remove the entire
        /// node and adjust the caret end.
        #[wasm_bindgen_test]
        fn remove_partial_image_node() {
            let mut ca = init(true);
            RemoveSelectionTest {
                children: vec![image_node(&ca), text_node(&ca, "a")],
                before: State { start: 0, end: image_node(&ca).html_size() - 5, nodes: 2 },
                after: State { start: 0, end: 0, nodes: 1 },
                removed: true,
            }.test(&mut ca);
            assert_eq!(nth_child(&ca, 0).text_content().unwrap(), "a");
        }
    }

    mod html_size {
        use super::*;

        #[wasm_bindgen_test]
        fn html_size_with_emoji() {
            let window = web_sys::window().expect("No global `window` exists");
            let document = window.document().expect("Should have a document on window");
            let img = document.create_element("img").unwrap();
            img.set_attribute("src", "test.jpg").unwrap();
            img.set_attribute("alt", "🍻").unwrap();
            img.set_attribute("class", "umläöüt").unwrap();
            let node: Node = img.unchecked_into();
            assert_eq!(node.html_size(), 45);
        }
    }

    mod insert_node {
        use super::*;

        struct InsertNodeTest<N> {
            html: String,
            caret_before: (u32, u32),
            node: N,
            caret_after: (u32, u32),
            final_html: String,
        }

        mod text {
            use super::*;

            impl InsertNodeTest<&'static str> {
                fn test(&self, ca: &mut ComposeArea) {
                    ca.get_wrapper().set_inner_html(&self.html);
                    ca.set_caret_position(self.caret_before.0, self.caret_before.1);
                    ca.set_dom_caret_position_from_state();

                    assert_eq!(ca.caret_start, self.caret_before.0);
                    assert_eq!(ca.caret_end, self.caret_before.1);

                    ca.insert_text(self.node);

                    assert_eq!(ca.caret_start, self.caret_after.0);
                    assert_eq!(ca.caret_end, self.caret_after.1);

                    assert_eq!(ca.get_wrapper().inner_html(), self.final_html);
                }
            }

            #[wasm_bindgen_test]
            fn at_end() {
                let mut ca = init(true);
                InsertNodeTest {
                    html: "hello ".into(),
                    caret_before: (6, 6),
                    node: "world",
                    caret_after: (11, 11),
                    final_html: "hello world".into(),
                }.test(&mut ca);
            }

            #[wasm_bindgen_test]
            fn in_the_middle() {
                let mut ca = init(true);
                InsertNodeTest {
                    html: "ab".into(),
                    caret_before: (1, 1),
                    node: "XY",
                    caret_after: (3, 3),
                    final_html: "aXYb".into(),
                }.test(&mut ca);
            }

            #[wasm_bindgen_test]
            fn replace_nodes() {
                let mut ca = init(true);
                let img = Img { src: "img.jpg", alt: "😀", cls: "em" };
                InsertNodeTest {
                    html: format!("ab{}", img.html()),
                    caret_before: (1, 1 + img.html_size()),
                    node: "z",
                    caret_after: (2, 2),
                    final_html: "az".into(),
                }.test(&mut ca);
            }
        }

        mod image {
            use super::*;

            impl InsertNodeTest<Img> {
                fn test(&self, ca: &mut ComposeArea) {
                    ca.get_wrapper().set_inner_html(&self.html);
                    ca.set_caret_position(self.caret_before.0, self.caret_before.1);
                    ca.set_dom_caret_position_from_state();

                    assert_eq!(ca.caret_start, self.caret_before.0);
                    assert_eq!(ca.caret_end, self.caret_before.1);

                    ca.insert_image(self.node.src, self.node.alt, self.node.cls);

                    assert_eq!(ca.caret_start, self.caret_after.0);
                    assert_eq!(ca.caret_end, self.caret_after.1);

                    assert_eq!(ca.get_wrapper().inner_html(), self.final_html);
                }
            }

            #[wasm_bindgen_test]
            fn at_end() {
                let mut ca = init(true);
                let img = Img { src: "img.jpg", alt: "😀", cls: "em" };
                InsertNodeTest {
                    html: "hi ".into(),
                    caret_before: (3, 3),
                    node: img,
                    caret_after: (3 + img.html_size(), 3 + img.html_size()),
                    final_html: format!("hi {}", img.html()),
                }.test(&mut ca);
            }

            #[wasm_bindgen_test]
            fn split_text() {
                let mut ca = init(true);
                let img = Img { src: "img.jpg", alt: "😀", cls: "em" };
                InsertNodeTest {
                    html: "bonjour".into(),
                    caret_before: (3, 3),
                    node: img,
                    caret_after: (3 + img.html_size(), 3 + img.html_size()),
                    final_html: format!("bon{}jour", img.html()),
                }.test(&mut ca);
            }

            #[wasm_bindgen_test]
            fn between_nodes_br() {
                let mut ca = init(true);
                let img = Img { src: "img.jpg", alt: "😀", cls: "em" };
                InsertNodeTest {
                    html: "a<br>b".into(),
                    caret_before: (1, 1),
                    node: img,
                    caret_after: (1 + img.html_size(), 1 + img.html_size()),
                    final_html: format!("a{}<br>b", img.html()),
                }.test(&mut ca);
            }
        }
    }

}
