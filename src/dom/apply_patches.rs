//! provides functionalities related to patching the DOM in the browser.
use crate::{
    dom::{
        created_node,
        created_node::{
            ActiveClosure,
            CreatedNode,
        },
    },
    prelude::AttributeValue,
    Dispatch,
    Patch,
};
use js_sys::Function;
use std::collections::{
    HashMap,
    HashSet,
};
use wasm_bindgen::{
    JsCast,
    JsValue,
};
use web_sys::{
    Element,
    HtmlInputElement,
    HtmlTextAreaElement,
    Node,
    Text,
};

/// Apply all of the patches to our old root node in order to create the new root node
/// that we desire.
/// This is usually used after diffing two virtual nodes.
///
/// Note: If Program is None, it is a dumb patch, meaning
/// there is no event listener attached or changed
pub fn patch<N, DSP, MSG>(
    program: Option<&DSP>,
    root_node: N,
    old_closures: &mut ActiveClosure,
    patches: &[Patch<MSG>],
) -> Result<ActiveClosure, JsValue>
where
    N: Into<Node>,
    MSG: 'static,
    DSP: Clone + Dispatch<MSG> + 'static,
{
    let root_node: Node = root_node.into();

    // Closure that were added to the DOM during this patch operation.
    let mut active_closures = HashMap::new();

    // finding the nodes to be patched before hand, instead of calling it
    // in every patch loop.
    let (element_nodes_to_patch, text_nodes_to_patch) =
        find_nodes(root_node, patches);

    for patch in patches {
        let patch_node_idx = patch.node_idx();

        if let Some(element) = element_nodes_to_patch.get(&patch_node_idx) {
            let new_closures =
                apply_element_patch(program, &element, old_closures, &patch)?;
            active_closures.extend(new_closures);
            continue;
        }

        if let Some(text_node) = text_nodes_to_patch.get(&patch_node_idx) {
            apply_text_patch(program, &text_node, &patch)?;
            continue;
        }

        unreachable!(
            "Getting here means we didn't find the element or next node that we were supposed to patch."
        )
    }

    Ok(active_closures)
}

/// find the nodes to be patched
/// each patch contains a node index, arranged in depth first tree.
///
/// This function is needed for optimization purposes.
/// Instead of finding the nodes each time in the patching process.
/// We find them before hand so as not to keep calling this function for each and every element to
/// be patched.
fn find_nodes<MSG>(
    root_node: Node,
    patches: &[Patch<MSG>],
) -> (HashMap<usize, Element>, HashMap<usize, Text>) {
    let mut cur_node_idx = 0;
    let mut nodes_to_find = HashSet::new();

    for patch in patches {
        nodes_to_find.insert(patch.node_idx());
    }

    find_nodes_recursive(root_node, &mut cur_node_idx, &nodes_to_find)
}

/// find the html nodes recursively
fn find_nodes_recursive(
    root_node: Node,
    cur_node_idx: &mut usize,
    nodes_to_find: &HashSet<usize>,
) -> (HashMap<usize, Element>, HashMap<usize, Text>) {
    let mut element_nodes_to_patch = HashMap::new();
    let mut text_nodes_to_patch = HashMap::new();

    // We use child_nodes() instead of children() because children() ignores text nodes
    let children = root_node.child_nodes();
    let child_node_count = children.length();

    // If the root node matches, mark it for patching
    if nodes_to_find.get(&cur_node_idx).is_some() {
        match root_node.node_type() {
            Node::ELEMENT_NODE => {
                element_nodes_to_patch
                    .insert(*cur_node_idx, root_node.unchecked_into());
            }
            Node::TEXT_NODE => {
                text_nodes_to_patch
                    .insert(*cur_node_idx, root_node.unchecked_into());
            }
            other => unimplemented!("Unsupported root node type: {}", other),
        }
    }

    *cur_node_idx += 1;

    for i in 0..child_node_count {
        let child_node = children.item(i).expect("Expecting a child node");

        match child_node.node_type() {
            Node::ELEMENT_NODE => {
                let child_to_patch = find_nodes_recursive(
                    child_node,
                    cur_node_idx,
                    nodes_to_find,
                );

                element_nodes_to_patch.extend(child_to_patch.0);
                text_nodes_to_patch.extend(child_to_patch.1);
            }
            Node::TEXT_NODE => {
                if nodes_to_find.get(&cur_node_idx).is_some() {
                    text_nodes_to_patch
                        .insert(*cur_node_idx, child_node.unchecked_into());
                }

                *cur_node_idx += 1;
            }
            Node::COMMENT_NODE => {
                // At this time we do not support user entered comment nodes, so if we see a comment
                // then it was a delimiter created by virtual-dom-rs in order to ensure that two
                // neighboring text nodes did not get merged into one by the browser. So we skip
                // over this virtual-dom-rs generated comment node.
            }
            _other => {
                // Ignoring unsupported child node type
                // TODO: What do we do with this situation? Log a warning?
            }
        }
    }

    (element_nodes_to_patch, text_nodes_to_patch)
}

/// Get the "data-sauron-vdom-id" of all the desendent of this node including itself
/// This is needed to free-up the closure that was attached ActiveClosure manually
/// TODO: Make a test when an element is removed, all of it's descendant closure should also be
/// removed as well.
fn get_node_descendant_data_vdom_id(root_element: &Element) -> Vec<u32> {
    let mut data_vdom_id = vec![];

    // TODO: there should be a better way to get the node-id back
    // without having to read from the actual dom node element
    if let Some(vdom_id_str) =
        root_element.get_attribute(created_node::DATA_SAURON_VDOM_ID)
    {
        let vdom_id = vdom_id_str
            .parse::<u32>()
            .expect("unable to parse sauron_vdom-id");
        data_vdom_id.push(vdom_id);
    }

    let children = root_element.child_nodes();
    let child_node_count = children.length();
    for i in 0..child_node_count {
        let child_node = children.item(i).expect("Expecting a child node");
        if let Node::ELEMENT_NODE = child_node.node_type() {
            let child_element = child_node.unchecked_ref::<Element>();
            let child_data_vdom_id =
                get_node_descendant_data_vdom_id(child_element);
            data_vdom_id.extend(child_data_vdom_id);
        }
    }
    data_vdom_id
}

/// remove all the event listeners for this node
fn remove_event_listeners(
    node: &Element,
    old_closures: &mut ActiveClosure,
) -> Result<(), JsValue> {
    let all_descendant_vdom_id = get_node_descendant_data_vdom_id(node);
    //crate::log!("all descendatant vdom_id: {:#?}", all_descendant_vdom_id);
    for vdom_id in all_descendant_vdom_id {
        //crate::log!("Removing listener for vdom_id: {}", vdom_id);

        let old_closure = old_closures
            .get(&vdom_id)
            .expect("There is no marked with that vdom_id");

        for (event, oc) in old_closure.iter() {
            let func: &Function = oc.as_ref().unchecked_ref();
            node.remove_event_listener_with_callback(event, func)?;
        }

        // remove closure active_closure in dom_updater to free up memory
        old_closures
            .remove(&vdom_id)
            .expect("Unable to remove old closure");
    }
    Ok(())
}

/// apply a the patch to this element node.
/// and return the ActiveClosure that may be attached to that element
fn apply_element_patch<DSP, MSG>(
    program: Option<&DSP>,
    node: &Element,
    old_closures: &mut ActiveClosure,
    patch: &Patch<MSG>,
) -> Result<ActiveClosure, JsValue>
where
    MSG: 'static,
    DSP: Clone + Dispatch<MSG> + 'static,
{
    let active_closures = ActiveClosure::new();
    match patch {
        Patch::AddAttributes(_tag, _node_idx, attributes) => {
            for attr in attributes.iter() {
                // attr "" is used in checked = false, since checked attribute is only unchecked
                // when there is no checked attribute
                if !attr.name().is_empty() {
                    // NOTE: set_attribute('value',..) is not enough
                    // value need to explicitly call the set_value in order for the
                    // actual value gets reflected.
                    //
                    // TODO: centrarlize this with set_attributes in created_node
                    match *attr.name() {
                        "value" => {
                            let string_value = match attr.value() {
                                AttributeValue::Simple(value) => {
                                    value.to_string()
                                }
                                _ => String::new(),
                            };
                            if let Some(input) =
                                node.dyn_ref::<HtmlInputElement>()
                            {
                                input.set_value(&string_value);
                            } else if let Some(textarea) =
                                node.dyn_ref::<HtmlTextAreaElement>()
                            {
                                textarea.set_value(&string_value);
                            }
                        }
                        "checked" => {
                            if let Some(input) =
                                node.dyn_ref::<HtmlInputElement>()
                            {
                                let checked = attr
                                    .value()
                                    .get_simple()
                                    .map(|v| v.as_bool())
                                    .flatten()
                                    .unwrap_or(false);
                                input.set_checked(checked);
                            }
                        }
                        "inner_html" => {
                            if let Some(element) = node.dyn_ref::<Element>() {
                                element.set_inner_html(
                                    &attr
                                        .value()
                                        .get_function_call_value()
                                        .map(|v| v.to_string())
                                        .unwrap_or(String::new()),
                                );
                            }
                        }
                        _ => {
                            node.set_attribute(
                                attr.name(),
                                &attr
                                    .value()
                                    .get_simple()
                                    .map(|v| v.to_string())
                                    .unwrap_or(String::new()),
                            )?;
                        }
                    }
                }
            }

            Ok(active_closures)
        }
        Patch::RemoveAttributes(_tag, _node_idx, attributes) => {
            for attrib_name in attributes.iter() {
                node.remove_attribute(attrib_name)?;
                //TODO: also explicitly deal with value here..
            }

            Ok(active_closures)
        }

        // THis also removes the associated closures and event listeners to the node being replaced
        // including the associated closures of the descendant of replaced node
        // before it is actully replaced in the DOM
        //
        Patch::Replace(_tag, _node_idx, new_node) => {
            let created_node = CreatedNode::<Node>::create_dom_node_opt::<
                DSP,
                MSG,
            >(program, new_node);
            remove_event_listeners(&node, old_closures)?;
            node.replace_with_with_node_1(&created_node.node)?;
            Ok(created_node.closures)
        }
        // This also removes the associated closures and event listener to the truncated chilren
        // before actually removing it from the DOM
        //
        // The browser will take handling of removing the event listeners
        // of the children and indirect children of this node ( so we don't have to manually remove
        // them).
        // The closures of descendant of the children is also removed
        Patch::TruncateChildren(_tag, _node_idx, num_children_remaining) => {
            let children = node.child_nodes();
            let child_count = children.length();

            // We skip over any separators that we placed between two text nodes
            //   -> `<!--mordor-->`
            //  and trim all children that come after our new desired `num_children_remaining`
            //let mut non_separator_children_found = 0;

            let to_be_remove_len =
                child_count as usize - num_children_remaining;
            for _index in 0..to_be_remove_len {
                let last_child = node.last_child().expect("No more last child");
                let last_element: &Element = last_child.unchecked_ref();
                remove_event_listeners(last_element, old_closures)?;
                // Do not remove comment node
                if last_child.node_type() == Node::COMMENT_NODE {
                    continue;
                }
                node.remove_child(&last_child)
                    .expect("Unable to remove last child");
            }

            Ok(active_closures)
        }
        Patch::AppendChildren(_tag, _node_idx, new_nodes) => {
            let parent = &node;
            let mut active_closures = HashMap::new();
            for new_node in new_nodes {
                let created_node = CreatedNode::<Node>::create_dom_node_opt::<
                    DSP,
                    MSG,
                >(program, &new_node);
                parent.append_child(&created_node.node)?;
                active_closures.extend(created_node.closures);
            }

            Ok(active_closures)
        }
        Patch::ChangeText(_node_idx, _new_node) => {
            unreachable!("Elements should not receive ChangeText patches.")
        }
    }
}

fn apply_text_patch<DSP, MSG>(
    program: Option<&DSP>,
    node: &Text,
    patch: &Patch<MSG>,
) -> Result<(), JsValue>
where
    MSG: 'static,
    DSP: Clone + Dispatch<MSG> + 'static,
{
    match patch {
        Patch::ChangeText(_node_idx, new_text) => {
            node.set_node_value(Some(&new_text));
        }
        Patch::Replace(_tag, _node_idx, new_node) => {
            let created_node = CreatedNode::<Node>::create_dom_node_opt::<
                DSP,
                MSG,
            >(program, new_node);
            node.replace_with_with_node_1(&created_node.node)?;
        }
        _other => {
            unreachable!(
                "Text nodes should only receive ChangeText or Replace patches."
            )
        }
    };

    Ok(())
}
