//! This contains a trait to be able to render
//! virtual dom into a writable buffer
//!
use crate::{
    html::attributes::AttributeValue, mt_dom::AttValue, Attribute, Element,
    Node,
};
use std::fmt;

/// render node, elements to a writable buffer
pub trait Render {
    /// render the node to a writable buffer
    fn render(&self, buffer: &mut dyn fmt::Write) -> fmt::Result {
        self.render_with_indent(buffer, 0, &mut Some(0))
    }
    /// render instance to a writable buffer with indention
    fn render_with_indent(
        &self,
        buffer: &mut dyn fmt::Write,
        indent: usize,
        node_idx: &mut Option<usize>,
    ) -> fmt::Result;
}

impl<MSG> Render for Node<MSG> {
    fn render_with_indent(
        &self,
        buffer: &mut dyn fmt::Write,
        indent: usize,
        node_idx: &mut Option<usize>,
    ) -> fmt::Result {
        match self {
            Node::Element(element) => {
                element.render_with_indent(buffer, indent, node_idx)
            }
            Node::Text(text) => write!(buffer, "{}", text),
        }
    }
}

impl<MSG> Render for Element<MSG> {
    fn render_with_indent(
        &self,
        buffer: &mut dyn fmt::Write,
        indent: usize,
        node_idx: &mut Option<usize>,
    ) -> fmt::Result {
        write!(buffer, "<{}", self.tag())?;

        let ref_attrs: Vec<&Attribute<MSG>> =
            self.get_attributes().iter().map(|att| att).collect();
        let merged_attributes: Vec<Attribute<MSG>> =
            mt_dom::merge_attributes_of_same_name(&ref_attrs);
        for attr in merged_attributes {
            write!(buffer, " ")?;
            attr.render_with_indent(buffer, indent, node_idx)?;
        }
        #[cfg(feature = "with-measure")]
        if let Some(node_idx) = node_idx {
            let node_idx_attr: Attribute<MSG> =
                crate::prelude::attr("node_idx", *node_idx);
            write!(buffer, " ")?;
            node_idx_attr.render_with_indent(buffer, indent, &mut None)?;
        }
        write!(buffer, ">")?;

        let children = self.get_children();
        let first_child = children.get(0);
        let is_first_child_text_node =
            first_child.map(|node| node.is_text()).unwrap_or(false);

        let is_lone_child_text_node =
            children.len() == 1 && is_first_child_text_node;

        // do not indent if it is only text child node
        if is_lone_child_text_node {
            node_idx.as_mut().map(|idx| *idx += 1);
            first_child
                .unwrap()
                .render_with_indent(buffer, indent, node_idx)?;
        } else {
            // otherwise print all child nodes with each line and indented
            for child in self.get_children() {
                node_idx.as_mut().map(|idx| *idx += 1);
                write!(buffer, "\n{}", "    ".repeat(indent + 1))?;
                child.render_with_indent(buffer, indent + 1, node_idx)?;
            }
        }
        // do not make a new line it if is only a text child node or it has no child nodes
        if !is_lone_child_text_node && !children.is_empty() {
            write!(buffer, "\n{}", "    ".repeat(indent))?;
        }
        write!(buffer, "</{}>", self.tag())?;
        Ok(())
    }
}

impl<MSG> Render for Attribute<MSG> {
    fn render_with_indent(
        &self,
        buffer: &mut dyn fmt::Write,
        indent: usize,
        node_idx: &mut Option<usize>,
    ) -> fmt::Result {
        write!(buffer, "{}=\"", self.name())?;
        for (i, att_value) in self.value().iter().enumerate() {
            match att_value {
                AttValue::Plain(plain) => {
                    if i > 0 && !plain.is_style() {
                        write!(buffer, " ")?;
                    }
                    plain.render_with_indent(buffer, indent, node_idx)?;
                }
                _ => (),
            }
        }
        write!(buffer, "\"")?;
        Ok(())
    }
}

impl Render for AttributeValue {
    fn render_with_indent(
        &self,
        buffer: &mut dyn fmt::Write,
        _index: usize,
        _node_idx: &mut Option<usize>,
    ) -> fmt::Result {
        match self {
            AttributeValue::Simple(simple) => {
                write!(buffer, "{}", simple.to_string())?;
            }
            AttributeValue::Style(styles_att) => {
                for s_att in styles_att {
                    write!(buffer, "{};", s_att)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn test_render_classes() {
        let view: Node<()> =
            div(vec![class("frame"), class("component")], vec![]);
        let expected = r#"<div class="frame component"></div>"#;
        let mut buffer = String::new();
        view.render(&mut buffer).expect("must render");
        assert_eq!(expected, buffer);
    }

    #[test]
    fn test_render_class_flag() {
        let view: Node<()> = div(
            vec![
                class("frame"),
                classes_flag([("component", true), ("layer", false)]),
            ],
            vec![],
        );
        let expected = r#"<div class="frame component"></div>"#;
        let mut buffer = String::new();
        view.render(&mut buffer).expect("must render");
        assert_eq!(expected, buffer);
    }
}
