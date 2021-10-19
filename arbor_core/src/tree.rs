use crate::ensure::ensure;
use fixedbitset::FixedBitSet;
use log::{debug, info, trace};
use nanoserde::{DeJson, SerJson};
use std::fmt;

/// Type definitions for Node, Edge, and placement indices resolved to usize, mainly used for
/// improved readability on whether a value should be used for edge or node access
pub type NodeIndex = usize;
pub type EdgeIndex = usize;
pub type PlacementIndex = usize;

/// Shim type definition. Used to provide type information for tree while nanoserde doesn't have
/// generics support
// TODO: Once nanoserde supports generics, move this into generic type for Tree
pub type N = crate::Dialogue;
pub type E = crate::Choice;

/// This trait implements an "end" value that may be used to signal an invalid value for
/// an element in the tree, such as a linked list. This should be used in places where Option
/// would result in extra memory usage (such as uint types)
pub trait End {
    fn end() -> Self;
}

impl End for usize {
    fn end() -> Self {
        NodeIndex::MAX
    }
}

/// Generic tree result type
type Result<T> = std::result::Result<T, Error>;

/// Tree error types
#[derive(Debug)]
pub enum Error {
    InvalidNodeIndex,
    InvalidEdgeIndex,
    NodeInUse,
    InvalidEdgeLinks,
    NodesFull,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::InvalidNodeIndex => write!(
                f,
                "Attempted to access a node that is not present in the tree"
            ),
            Error::InvalidEdgeIndex => write!(
                f,
                "Attempted to access an edge that is not present in the tree"
            ),
            Error::NodeInUse => write!(
                f,
                "Modification cannot be made to node as it is currently in use in the tree"
            ),
            Error::InvalidEdgeLinks => write!(
                f,
                "Attempted to access an invalid edge in an outgoing edges linked list"
            ),
            Error::NodesFull => write!(
                f,
                "Nodes list full, node list cannot be larger than usize::MAX - 1"
            ),
        }
    }
}

/// Modifying events that occur in the tree. These are returned by methods that cause the given
/// event. Event structs store the data needed to reconstruct the event after the fact
pub mod event {
    use super::{EdgeIndex, NodeIndex, PlacementIndex, E, N};

    /// Information about a node insertion such that the event can be reconstructed
    ///
    /// This structure is returned by methods in the tree module that perform an equivalent event
    #[derive(Debug)]
    pub struct NodeInsert {
        pub index: NodeIndex,
        pub node: N,
    }

    /// Information about a node removal such that the event can be reconstructed
    ///
    /// This structure is returned by methods in the tree module that perform an equivalent event
    ///
    #[derive(Debug)]
    pub struct NodeRemove {
        pub index: NodeIndex,
        pub node: N,
    }

    /// Information about a node edit such that the event can be reconstructed
    ///
    /// This structure is returned by methods in the tree module that perform an equivalent event
    #[derive(Debug)]
    pub struct NodeEdit {
        pub index: NodeIndex,
        pub from: N,
        pub to: N,
    }

    /// Information about an edge insertion such that the event can be reconstructed
    ///
    /// This structure is returned by methods in the tree module that perform an equivalent event
    #[derive(Debug)]
    pub struct EdgeInsert {
        pub source: NodeIndex,
        pub target: NodeIndex,
        pub index: EdgeIndex,
        pub placement: PlacementIndex,
        pub edge: E,
    }

    /// Information about an edge removal such that the event can be reconstructed
    ///
    /// This structure is returned by methods in the tree module that perform an equivalent event
    #[derive(Debug)]
    pub struct EdgeRemove {
        pub source: NodeIndex,
        pub target: NodeIndex,
        pub index: EdgeIndex,
        pub placement: PlacementIndex,
        pub edge: E,
    }

    /// Information about a edge edit such that the event can be reconstructed
    ///
    /// This structure is returned by methods in the tree module that perform an equivalent event
    #[derive(Debug)]
    pub struct EdgeEdit {
        pub index: EdgeIndex,
        pub from: E,
        pub to: E,
    }

    /// Information about a move of an edge from one placement in the outgoing edges linked
    /// list to another
    ///
    /// This structure is returned by methods in the tree module that perform an equivalent event
    #[derive(Debug)]
    pub struct LinkMove {
        pub source: NodeIndex,
        pub index: EdgeIndex,
        pub from: PlacementIndex,
        pub to: PlacementIndex,
    }
}

/// Iterator over the outgoing edge indices of a node
///
/// This structure is returned by methods in the tree module that perform an equivalent event
#[derive(Clone, Copy)]
pub struct OutgoingEdges<'a> {
    edge_links: &'a [EdgeIndex],
    next: EdgeIndex,
}

impl<'a> Iterator for OutgoingEdges<'a> {
    type Item = EdgeIndex;
    fn next(&mut self) -> Option<Self::Item> {
        // save self.next as the current index to return
        if self.next == EdgeIndex::end() {
            None
        } else {
            let current = self.next;
            self.next = self.edge_links[self.next];
            Some(current)
        }
    }
}

/// Walker for mutable references to the outgoing edges of a node. This takes a mutable
/// reference to the tree only on each call to a member method, and so allows for traversal and
/// modification of the tree simultaneously.
///
/// There is no guarantee that the walker will be valid if the graph is modified while the
/// EdgeWalker is alive. Do not modify the graph while an EdgeWalker is active
#[derive(Clone, Copy)]
pub struct OutgoingEdgeWalker {
    /// Special condition for the first state of the walker, where it should return a reference
    /// to node_links rather than edge_links
    first: bool,
    /// Source node for outgoing edges. This allows the walker to capture the first reference
    /// to node_links
    source: NodeIndex,
    /// The current index into edge_links of the walker. This is ignored in the 'first' state
    /// of the walker.
    pub current: EdgeIndex,
    /// The next index in the edge_links of the walker. Will be None if the walker is at the
    /// end of the linked list
    next: Option<EdgeIndex>,
    /// current placement of walker in linked list
    pub placement: PlacementIndex,
}

impl OutgoingEdgeWalker {
    /// create a new outgoing edge walker object for a given source node
    ///
    /// # Errors
    /// Error if node index is invalid
    pub fn new(tree: &Tree, source: NodeIndex) -> Result<Self> {
        // current starts out as 0 as it is unused when 'first' is true in the initial
        // state. After the first call to next(), current will be loaded with a valid EdgeIndex
        let current = 0;

        // Get next link from node_links, if the node is invalid error out, if the node_link is
        // end() then set next to None so the walker will terminate immediately with
        // placment == 0
        let mut next = Some(*tree.node_links.get(source).ok_or(Error::InvalidNodeIndex)?);

        if next.unwrap() == NodeIndex::end() {
            next = None;
        }

        Ok(Self {
            first: true,
            source,
            current,
            next,
            placement: 0,
        })
    }

    /// Move the EdgeWalker to the next position in the outgoing edges list.
    ///
    /// Returns a tuple of a mutable reference to the current node_links or edge_links entry
    /// and a boolean indicating if the walker has reached the end of the linked list. Calling
    /// next repeatedly after the walker has reached the end of the list will return the same
    /// mutable reference to the last entry, and boolean = true.
    ///
    // NOTE: The goal is to implement next without any branches if possible. My initial attempt
    // to do this uses exclusively conditional assignment operations to handle differing
    // behavior between situations where the current position is in the node_links array vs
    // the edge_links array
    pub fn next<'a>(&mut self, tree: &'a mut Tree) -> Result<(&'a mut EdgeIndex, bool)> {
        // get node and edge link as well as possible next values each time, decide which to
        // return based on 'first'
        // this is done via matched assigns to ensure conditional movs rather than branches
        let next_from_edge = tree
            .edge_links
            .get(self.next.unwrap_or(EdgeIndex::MAX))
            .copied()
            .unwrap_or(EdgeIndex::MAX);
        let node_link = tree
            .node_links
            .get_mut(self.source)
            .ok_or(Error::InvalidNodeIndex)?;
        let next_from_node = Some(*node_link);
        let edge_link = tree.edge_links.get_mut(self.current).unwrap();

        let current_link = match self.first {
            true => node_link,
            false => edge_link,
        };

        // Walk tree based on whether self.next is valid
        //
        // get a boolean corresponding to whether we are at the end of the list
        let at_end = self.next.is_none();
        // conditionally clear 'first' state if we are not at the end of the list
        // If first has already been cleared by a prior iteration, it will remain false
        self.first &= at_end;
        // go to next position in the linked list, stop if we are at the end already
        self.current = match self.next {
            Some(v) => v,
            None => self.current,
        };
        // increment placement walker if we aren't at the end of the linked list
        self.placement += match self.next {
            Some(_) => 1,
            None => 0,
        };

        // convert from EdgeIndex::end() to None for potential next indices
        let next_option_from_edge = match next_from_edge {
            EdgeIndex::MAX => None,
            _ => Some(next_from_edge),
        };

        // update self.next, should end up being None if the list is over
        // uses the potential next indices that we captured at the beginning of the method
        // prior to mutably borrowing any links
        self.next = match self.first {
            true => next_from_node,
            false => next_option_from_edge,
        };

        Ok((current_link, at_end))
    }

    /// skip n links, and return the next one. If the skip goes past the end of the list the
    /// last link will be returned
    pub fn skip<'a>(&mut self, tree: &'a mut Tree, n: PlacementIndex) -> Result<&'a mut EdgeIndex> {
        for _i in 0..n {
            self.next(tree)?;
        }
        let (link_reference, _) = self.next(tree)?;
        Ok(link_reference)
    }

    /// Go to the last link in the outgoing edges and return a mutable pointer to it.
    pub fn last<'a>(&mut self, tree: &'a mut Tree) -> Result<&'a mut EdgeIndex> {
        while !self.next(tree)?.1 {}
        let (link_reference, _) = self.next(tree)?;
        Ok(link_reference)
    }
}

#[derive(Debug, SerJson, DeJson, Clone)]
pub struct Tree {
    // TODO: Make Node type generic if needed
    pub nodes: Vec<N>,
    pub edges: Vec<E>,
    /// Node links implement a linked list to the outgoing edges of a given node. The
    /// node index may be used to index into this array to get the first outgoing edge for that
    /// node.
    pub node_links: Vec<EdgeIndex>,
    /// Edge links implement a linked lists to rest of the outgoing edges of a given node. The
    /// edge index from the previous node_links or edge_links value may be used to index into
    /// this array to get the next outgoing edge for a given node.
    pub edge_links: Vec<EdgeIndex>,
    /// List of the sources of an edge. Access via an edge index to get the target node index
    /// for that edge.
    ///
    /// Stored separately to avoid wrapping the node type in the array.
    pub edge_sources: Vec<NodeIndex>,
    /// List of the targets of an edge. Access via an edge index to get the target node index
    /// for that edge.
    ///
    /// Stored separately to avoid wrapping the node type in the array.
    pub edge_targets: Vec<NodeIndex>,
}

impl Tree {
    /// Create a tree with allocation for a given number of nodes and edges
    pub fn with_capacity(node_capacity: usize, edge_capacity: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(node_capacity as usize),
            edges: Vec::with_capacity(edge_capacity as usize),
            node_links: Vec::with_capacity(node_capacity as usize),
            edge_links: Vec::with_capacity(edge_capacity as usize),
            edge_sources: Vec::with_capacity(edge_capacity as usize),
            edge_targets: Vec::with_capacity(edge_capacity as usize),
        }
    }

    /// Clear the contents of a tree, reset all internal data
    #[inline]
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.node_links.clear();
        self.edge_links.clear();
        self.edge_sources.clear();
        self.edge_targets.clear();
    }

    /// Get the contents of a node
    ///
    /// # Errors
    ///
    /// Error if node index is invalid
    #[inline]
    pub fn get_node(&self, index: NodeIndex) -> Result<&N> {
        let node = self.nodes.get(index).ok_or(Error::InvalidNodeIndex)?;
        Ok(node)
    }

    /// Get the mutable contents of a node
    ///
    /// # Errors
    ///
    /// Error if node index is invalid
    #[inline]
    pub fn get_node_mut(&mut self, index: NodeIndex) -> Result<&mut N> {
        self.nodes.get_mut(index).ok_or(Error::InvalidNodeIndex)
    }

    /// Push a new node onto the tree, and return the index of the added node
    ///
    /// # Errors
    /// Error if the nodes list is full (more than usize::MAX - 1 nodes)
    #[inline]
    pub fn add_node(&mut self, node: N) -> Result<event::NodeInsert> {
        ensure(self.nodes.len() < NodeIndex::end() - 1, Error::NodesFull)?;
        self.nodes.push(node);
        self.node_links.push(EdgeIndex::end());

        // Create and return event information
        let event = event::NodeInsert {
            index: self.nodes.len() - 1,
            node,
        };

        Ok(event)
    }

    /// Edit the contents in an existing node and return the old contents.
    ///
    /// # Errors
    ///
    /// If the index is invalid, a corresponding error will be returned with no modification to
    /// the tree.
    #[inline]
    pub fn edit_node(&mut self, index: NodeIndex, new_node: N) -> Result<event::NodeEdit> {
        trace!("attempt to get mutable weight from node index");
        let node = self.nodes.get_mut(index).ok_or(Error::InvalidNodeIndex)?;
        let old_node_value = *node;

        *node = new_node;

        // Create and return event information
        let event = event::NodeEdit {
            index,
            from: old_node_value,
            to: *node,
        };
        Ok(event)
    }

    /// Remove a node if no edges use it as the source or target. Returns the weight of the
    /// removed node
    ///
    /// # Errors
    ///
    /// If the index is invalid, or if an edge currently uses the node as a source or target,
    /// an error is returned with no modification to the tree
    pub fn remove_node(&mut self, index: NodeIndex) -> Result<event::NodeRemove> {
        info!("Remove node {}", index);

        trace!("check that node index is valid");
        self.nodes.get(index).ok_or(Error::InvalidNodeIndex)?;

        let mut node_in_use = false;
        trace!("check that node has no outgoing edges");
        // faster than searching edge_sources
        node_in_use |= self.node_links[index] != NodeIndex::end();
        trace!("check that node is not the target of any edges");
        node_in_use |= self.edge_targets.contains(&index);
        if node_in_use {
            Err(Error::NodeInUse)
        } else {
            // capture the index of the node that is going to be swapped in (always the last
            // node index of the list)
            let swapped_index = self.nodes.len() - 1;

            trace!("swap remove node from nodes list and node_links");
            let removed_node = self.nodes.swap_remove(index);
            self.node_links.swap_remove(index);

            trace!("re-point edge sources and targets to the newly swapped node");
            for source in self.edge_sources.as_mut_slice() {
                if *source == swapped_index {
                    *source = index;
                }
            }

            for target in self.edge_targets.as_mut_slice() {
                if *target == swapped_index {
                    *target = index;
                }
            }
            // Create and return event information
            let event = event::NodeRemove {
                index,
                node: removed_node,
            };
            Ok(event)
        }
    }

    /// Insert a node in a specific location. Generally used to 'undo' a node removal
    /// operation. If the requested index is longer than the nodes list, it is placed at the
    /// end of the list. Returns the node_index where the node was inserted
    ///
    /// # Error
    ///
    /// Error if the node index is invalid or if the insertion fails
    pub fn insert_node(&mut self, node: N, desired_index: NodeIndex) -> Result<event::NodeInsert> {
        info!("Insert node at {}", desired_index);

        // clamp index by nodes list length
        let clamped_desired = std::cmp::min(desired_index, self.nodes.len());
        debug!("clamped index {} to {}", desired_index, clamped_desired);

        trace!("add node to end of nodes list");
        let new_node_data = self.add_node(node)?;
        let swap_index = new_node_data.index;

        info!("swap added node with node at the clamped desired index");
        self.nodes.swap(swap_index, clamped_desired);

        info!("resolve any edge sources/targets that have changed due to the swap");

        for source in self.edge_sources.as_mut_slice() {
            if *source == swap_index {
                *source = clamped_desired
            }
        }
        for target in self.edge_targets.as_mut_slice() {
            if *target == swap_index {
                *target = clamped_desired
            }
        }

        let event = event::NodeInsert {
            index: clamped_desired,
            node: new_node_data.node,
        };
        Ok(event)
    }

    /// Get an immutable slice of the nodes in the tree
    #[inline]
    pub fn nodes(&self) -> &[N] {
        self.nodes.as_slice()
    }

    /// Get an mutable slice of the nodes in the tree
    #[inline]
    pub fn nodes_mut(&mut self) -> &mut [N] {
        self.nodes.as_mut_slice()
    }

    /// Get the contents of an edge
    ///
    /// # Errors
    ///
    /// Error if edge index is invalid
    #[inline]
    pub fn get_edge(&self, index: EdgeIndex) -> Result<&E> {
        self.edges.get(index).ok_or(Error::InvalidEdgeIndex)
    }

    /// Get the mutable contents of an edge
    ///
    /// # Errors
    ///
    /// Error if edge index is invalid
    #[inline]
    pub fn get_edge_mut(&mut self, index: EdgeIndex) -> Result<&mut E> {
        self.edges.get_mut(index).ok_or(Error::InvalidEdgeIndex)
    }

    /// Get the source node index of an edge
    #[inline]
    pub fn source_of(&self, edge_index: EdgeIndex) -> Result<NodeIndex> {
        self.edge_sources
            .get(edge_index)
            .copied()
            .ok_or(Error::InvalidEdgeIndex)
    }

    /// Get the target node index of an edge
    #[inline]
    pub fn target_of(&self, edge_index: EdgeIndex) -> Result<NodeIndex> {
        self.edge_targets
            .get(edge_index)
            .copied()
            .ok_or(Error::InvalidEdgeIndex)
    }

    /// Get the placement of an edge in the outgoing_edges linked list of a source node
    ///
    /// # Errors
    /// Error if indices are invalid or if edge is not ougoing from source
    #[inline]
    pub fn placement_of(&self, source: NodeIndex, index: EdgeIndex) -> Result<PlacementIndex> {
        let (placement, _edge) = self
            .outgoing_from_index(source)?
            .enumerate()
            .find(|(_i, e)| *e == index)
            .ok_or(Error::InvalidEdgeLinks)?;
        Ok(placement)
    }

    /// Create a new edge from a source node to a target node, return the index of the added edge
    ///
    /// # Errors
    ///
    /// If either the source or target node is invalid, a corresponding error will be returned
    /// with no modification to the tree.
    ///
    /// # Panic
    ///
    /// Panics if a cycle is found in the edge_links list for this node. This means that the
    /// graph is corrupted and likely can't be recovered
    pub fn add_edge(
        &mut self,
        source: NodeIndex,
        target: NodeIndex,
        edge: E,
    ) -> Result<event::EdgeInsert> {
        trace!("check validity of source and target node");
        self.nodes.get(source).ok_or(Error::InvalidNodeIndex)?;
        self.nodes.get(target).ok_or(Error::InvalidNodeIndex)?;

        trace!("push new edge to the edges, edge_links, and edge_targets list");
        self.edges.push(edge.clone());
        self.edge_sources.push(source);
        self.edge_targets.push(target);
        self.edge_links.push(EdgeIndex::end());

        let new_edge_index = self.edges.len() - 1;

        trace!("update outgoing edges list for source node");
        // get a mutable reference to the last entry in the linked list
        let mut walker = OutgoingEdgeWalker::new(self, source)?;
        let last = walker.last(self)?;

        // double check that this link is actually end of the list
        debug!("end link value is: {}", *last);
        *last = new_edge_index;

        let event = event::EdgeInsert {
            source,
            target,
            index: new_edge_index,
            placement: walker.placement,
            edge,
        };
        Ok(event)
    }

    /// Edit the choice in an existing edge. The source or target node cannot be modified, the
    /// edge will have to be deleted and readded
    ///
    /// # Errors
    ///
    /// If the index is invalid, a corresponding error will be returned
    /// with no modification to the tree.
    pub fn edit_edge(&mut self, index: EdgeIndex, new_edge: E) -> Result<event::EdgeEdit> {
        trace!("check validity of edge index");
        let edge = self.edges.get_mut(index).ok_or(Error::InvalidEdgeIndex)?;

        let old_edge = edge.clone();
        *edge = new_edge.clone();

        let event = event::EdgeEdit {
            index,
            from: old_edge,
            to: new_edge,
        };
        Ok(event)
    }

    /// Remove an existing edge from the tree and return a tuple of the source node, target node,
    /// Choice, and placement within the outgoing_edges linked list
    ///
    /// Removing edges invalidates edge indices
    ///
    /// # Errors
    ///
    /// If the index is invalid, an error will be returned without modifying the tree
    pub fn remove_edge(&mut self, index: EdgeIndex) -> Result<event::EdgeRemove> {
        trace!("check validity of edge index");
        self.edges.get(index).ok_or(Error::InvalidEdgeIndex)?;

        // get source and target of edge to return at end of fn
        let source = self.source_of(index)?;
        let target = self.target_of(index)?;

        // get placement in the ougoing edges linked_list to return at end of fn
        let placement = self.placement_of(source, index)?;
        trace!("redirect any node or edge links pointing to the edge about to be removed");
        // TODO: Could this safely be combined with the for loop through the list that happens
        // after the removal?
        for link in self.node_links.as_mut_slice() {
            if *link == index {
                // link should point to whatever the to-be-deleted link currently points to
                *link = self.edge_links[index];
            }
        }
        for link_index in 0..self.edge_links.len() {
            if self.edge_links[link_index] == index {
                // link should point to whatever the to-be-deleted link currently points to
                self.edge_links[link_index] = self.edge_links[index];
            }
        }

        // capture the index of the edge that is going to be swapped in (always the last
        // node index of the list). This edge need to be updated in the node_links and
        // edge_links after swap-removing the edge
        let swapped_index = self.edges.len() - 1;

        trace!("swap remove from edges, edge_links, and edge_targets");
        let removed_edge = self.edges.swap_remove(index);
        self.edge_links.swap_remove(index);
        self.edge_sources.swap_remove(index);
        self.edge_targets.swap_remove(index);

        trace!("update indices in node_links and edge_links for last edge index that was swapped");
        for link in self.node_links.as_mut_slice() {
            if *link == swapped_index {
                // link should point to the index that the edge was swapped into
                *link = index;
            }
        }
        for link in self.edge_links.as_mut_slice() {
            if *link == swapped_index {
                // link should point to the index that the edge was swapped into
                *link = index;
            }
        }

        let event = event::EdgeRemove {
            source,
            target,
            index,
            placement,
            edge: removed_edge,
        };
        Ok(event)
    }

    /// Insert an in a specific location in the edge list and with a specific placement in a
    /// given linked list of outgoing edges from a node. Generally used to 'undo' a edge
    /// removal operation. If the requested index is longer than the edges list, it is placed
    /// at the end of the list. Returns the edge_index where the edge was inserted and the
    /// placement of the edge in the linked list of its source node
    ///
    /// # Error
    ///
    /// Error if any indexes are invalid or if the insertion fails
    pub fn insert_edge(
        &mut self,
        source: NodeIndex,
        target: NodeIndex,
        edge: E,
        desired_index: EdgeIndex,
        desired_placement: PlacementIndex,
    ) -> Result<event::EdgeInsert> {
        info!(
            "Insert edge from {} to {} at index {} and placement {}",
            source, target, desired_index, desired_placement
        );

        // clamp index by nodes list length
        let clamped_desired_index = std::cmp::min(desired_index, self.nodes.len());
        debug!(
            "clamped index {} to {}",
            desired_index, clamped_desired_index
        );

        trace!("add edge to end of lists");
        let new_edge_data = self.add_edge(source, target, edge)?;
        let new_edge = new_edge_data.edge;
        let swap_index = new_edge_data.index;

        trace!("swap edge to desired index");
        self.edges.swap(swap_index, clamped_desired_index);
        self.edge_sources.swap(swap_index, clamped_desired_index);
        self.edge_links.swap(swap_index, clamped_desired_index);
        self.edge_targets.swap(swap_index, clamped_desired_index);

        trace!("resolve any node/edge links that have changed due to the swap");
        for link in self.node_links.as_mut_slice() {
            if *link == swap_index {
                *link = clamped_desired_index;
            } else if *link == clamped_desired_index {
                *link = swap_index;
            }
        }
        for link in self.edge_links.as_mut_slice() {
            if *link == swap_index {
                *link = clamped_desired_index;
            } else if *link == clamped_desired_index {
                *link = swap_index;
            }
        }

        trace!("change the placement of the edge in the source nodes' outgoing edges list");
        let edge_move_event =
            self.edit_link_order(source, clamped_desired_index, desired_placement)?;

        let event = event::EdgeInsert {
            source,
            target,
            index: clamped_desired_index,
            placement: edge_move_event.to,
            edge: new_edge,
        };
        Ok(event)
    }

    /// Edit the link order of an edge. This modifies where an edge appears in the linked list
    /// of outgoing edges from its source node. This is useful if a given edge needs to appear
    /// in a specific ordering when accessing the outgoing edges of a node
    ///
    /// returns the new placement of the edge
    /// Desired placement should be considered the index of the node links. 0 is the first
    /// placement. If the desired_placement given is larger than the number of outgoing edges,
    /// the edge is placed at the end of the linked list
    ///
    /// # Errors
    ///
    /// Error if the node index is invalid, or if the edge index is not an outgoing edge of
    /// source
    pub fn edit_link_order(
        &mut self,
        source: NodeIndex,
        index: EdgeIndex,
        desired_placement: PlacementIndex,
    ) -> Result<event::LinkMove> {
        let current_placement = self.placement_of(source, index)?;
        info!(
            "Edit link order for edge {} from {} to {}",
            index, current_placement, desired_placement,
        );

        trace!("remove link from list first");
        let current_edge_link = self.edge_links[index];
        // Check node_links first then edge_links
        for link in self.node_links.as_mut_slice() {
            if *link == index {
                // link should point to whatever the to-be-deleted link currently points to
                *link = current_edge_link;
            }
        }

        for link in self.edge_links.as_mut_slice() {
            if *link == index {
                // link should point to whatever the to-be-deleted link currently points to
                *link = current_edge_link;
            }
        }

        let new_placement = self.insert_link(source, index, desired_placement)?;

        let event = event::LinkMove {
            index,
            source,
            from: current_placement,
            to: new_placement,
        };
        Ok(event)
    }

    /// Private helper function that inserts an existing edge into the desired placement of a
    /// source node's outgoing edges linked list. Returns the placement of the edge in the
    /// linked list
    ///
    ///
    /// Implementation notes:
    ///  uses placement walker to resolve node_links and edge links lists, if placement is 0
    ///  then its the node_links that needs to change not the edge_links.
    ///
    ///  placement walker skips desired_placement number of links, then returns a mutable
    ///  reference to the next link.
    ///
    ///  example visualization:
    ///   we want to insert edge 2 in desired_placement=2:
    ///
    ///  ```text
    ///      source -> 0 -> 1 -> 3 -> end
    ///       N = 0_|    |    |
    ///           1______|    |
    ///           2___________|
    ///  ```
    ///  where N is the placement of a link in the linked list order
    ///  so if desired_placement = 2, edge should be inserted after edge 1, which is the
    ///  same as skipping 2 links and then pointing link at placement (N=2) at the
    ///  edge_index.
    ///
    ///  The edge_links[edge_index] should now point to whatever link N=2 was previously,
    ///  resulting in:
    ///  ```text
    ///      source -> 0 -> 1 -> 2 -> 3 -> end
    ///       N = 0_|    |    |
    ///           1______|    |
    ///           2___________|
    ///  ```
    ///  where only link at placement (N=2) and link[edge_index] were modified
    fn insert_link(
        &mut self,
        source: NodeIndex,
        index: EdgeIndex,
        desired_placement: PlacementIndex,
    ) -> Result<PlacementIndex> {
        info!(
            "insert edge {} into linked list of {} at placement {}",
            index, source, desired_placement
        );
        // get length of edge_links list, also checks that source is valid
        let len = self.outgoing_from_index(source)?.count();

        // clamp desired placement to length of linked_list
        let clamped_desired = std::cmp::min(len, desired_placement);
        debug!(
            "clamped desired placement from {} to {}",
            desired_placement, clamped_desired
        );

        trace!("insert the link at clamped desired location");
        let mut placement_walker = OutgoingEdgeWalker::new(self, source)?;
        let link_at_placement: &mut EdgeIndex = placement_walker.skip(self, clamped_desired)?;
        let val_at_placement = *link_at_placement;
        *link_at_placement = index;
        self.edge_links[index] = val_at_placement;

        Ok(clamped_desired)
    }

    /// Get an immutable view of the edges in the tree
    pub fn edges(&self) -> &[E] {
        self.edges.as_slice()
    }

    /// Get the outgoing edges from a node by index
    ///
    /// # Errors
    ///
    /// Error if index is invalid
    ///
    /// # Examples
    ///
    /// ```
    /// # use arbor_core::{Dialogue, Choice, ReqKind, EffectKind, Section};
    /// # use arbor_core::tree::{self, EdgeIndex, event, Tree};
    /// # let dialogue = Dialogue::new(Section::new([0, 0], 0));
    /// # let choice = Choice::new(Section::new([0,0],0), ReqKind::No, EffectKind::No);
    /// let mut tree = Tree::with_capacity(10, 10);
    /// // add two nodes with dummy dialogue values
    /// let first_node_event: event::NodeInsert = tree.add_node(dialogue).unwrap();
    /// let second_node_event: event::NodeInsert = tree.add_node(dialogue).unwrap();
    ///
    /// // create two edges from first_node with dummy choice value
    /// let first_edge_event: event::EdgeInsert = tree.add_edge(
    ///     first_node_event.index,
    ///     second_node_event.index,
    ///     choice.clone()).unwrap();
    /// let second_edge_event: event::EdgeInsert = tree.add_edge(
    ///     first_node_event.index,
    ///     second_node_event.index,
    ///     choice).unwrap();
    ///
    /// let outgoing_edges: Vec<EdgeIndex> = tree
    ///     .outgoing_from_index(first_node_event.index)
    ///     .unwrap()
    ///     .collect();
    /// assert_eq!(outgoing_edges, vec![0, 1]);
    /// ```
    #[inline]
    pub fn outgoing_from_index(&self, index: NodeIndex) -> Result<OutgoingEdges> {
        self.nodes.get(index).ok_or(Error::InvalidNodeIndex)?;
        Ok(OutgoingEdges {
            edge_links: self.edge_links.as_slice(),
            next: self.node_links[index],
        })
    }
}

/// Modified from https://docs.rs/petgraph/0.5.1/src/petgraph/visit/mod.rs.html#582
/// A mapping for storing the visited status for NodeId `N`.
pub trait VisitMap<N> {
    /// Mark `a` as visited.
    ///
    /// Return **true** if this is the first visit, false otherwise.
    fn visit(&mut self, a: N) -> bool;

    /// Return whether `a` has been visited before.
    fn is_visited(&self, a: &N) -> bool;
}

impl VisitMap<usize> for FixedBitSet {
    fn visit(&mut self, x: usize) -> bool {
        !self.put(x)
    }
    fn is_visited(&self, x: &usize) -> bool {
        self.contains(*x)
    }
}

/// Depth first search tree walker
/// Adapted from https://docs.rs/petgraph/0.5.1/src/petgraph/visit/traversal.rs.html#37
pub struct Dfs {
    /// stack of nodes to visit
    pub stack: Vec<NodeIndex>,
    /// Mapping of visited nodes
    pub discovered: FixedBitSet,
}

impl Dfs {
    #[inline]
    pub fn new(tree: &Tree, start: NodeIndex) -> Self {
        let mut dfs = Self {
            stack: Vec::with_capacity(tree.nodes.len()),
            discovered: FixedBitSet::with_capacity(tree.nodes.len()),
        };
        dfs.stack.push(start);
        dfs
    }

    /// Return the next node in the dfs. Returns None if the the traversal is done
    ///
    /// # Errors
    ///
    /// Error if any node index is invalid, this would be unexpected if root node is valid and
    /// tree isn't corrupted
    pub fn next(&mut self, tree: &Tree) -> Result<Option<NodeIndex>> {
        while let Some(node_index) = self.stack.pop() {
            if self.discovered.visit(node_index) {
                for edge_index in tree.outgoing_from_index(node_index)? {
                    let target_node_index = tree.target_of(edge_index)?;
                    if !self.discovered.is_visited(&target_node_index) {
                        self.stack.push(target_node_index);
                    }
                }
                return Ok(Some(node_index));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod test {
    use super::{EdgeIndex, Tree};
    use crate::{Choice, Dialogue, EffectKind, ReqKind, Section};
    #[test]
    fn outgoing_edges() {
        let mut tree = Tree::with_capacity(10, 10);
        //dummy dialogue for creating nodes
        let dia = Dialogue::new(Section::new([0, 0], 0));
        let choice = Choice::new(Section::new([0, 0], 0), ReqKind::No, EffectKind::No);

        for _ in 0..10 {
            tree.add_node(dia).unwrap();
        }

        // add edges such that all edges are an outgoing edge of node 0
        for i in 0..10 {
            tree.add_edge(0, i, choice.clone()).unwrap();
        }

        // iterate over all outgoing edges of node 0 and verify they are correct
        let outgoing_edges: Vec<EdgeIndex> = tree.outgoing_from_index(0).unwrap().collect();

        assert_eq!(outgoing_edges, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    /// Test adding, removing, then re-inserting nodes
    #[test]
    fn add_remove_node() {
        let mut tree = Tree::with_capacity(10, 10);
        //dummy dialogue for creating nodes
        let dia = Dialogue::new(Section::new([0, 0], 0));

        for _ in 0..10 {
            tree.add_node(dia).unwrap();
        }

        let tree_full = tree.clone();

        let event = tree.remove_node(5).unwrap();
        tree.insert_node(event.node, event.index).unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));

        let event = tree.remove_node(9).unwrap();
        let event = tree.insert_node(event.node, event.index).unwrap();
        let event = tree.remove_node(event.index).unwrap();
        let _event = tree.insert_node(event.node, event.index).unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));

        let event = tree.remove_node(0).unwrap();
        tree.insert_node(event.node, event.index).unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));
    }

    /// Test adding, removing, then re-inserting edges
    #[test]
    fn add_remove_edge() {
        let mut tree = Tree::with_capacity(10, 10);
        //dummy dialogue for creating nodes
        let dia = Dialogue::new(Section::new([0, 0], 0));
        let choice = Choice::new(Section::new([0, 0], 0), ReqKind::No, EffectKind::No);

        for _ in 0..10 {
            tree.add_node(dia).unwrap();
        }

        // add edges such that all edges are an outgoing edge of node 0
        for i in 0..10 {
            tree.add_edge(0, i, choice.clone()).unwrap();
        }
        let tree_full = tree.clone();

        let event = tree.remove_edge(5).unwrap();

        tree.insert_edge(
            event.source,
            event.target,
            event.edge,
            event.index,
            event.placement,
        )
        .unwrap();
        assert_eq!(format!("{:#?}", tree), format!("{:#?}", tree_full),);

        let event = tree.remove_edge(0).unwrap();
        tree.insert_edge(
            event.source,
            event.target,
            event.edge,
            event.index,
            event.placement,
        )
        .unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));

        let event = tree.remove_edge(9).unwrap();
        tree.insert_edge(
            event.source,
            event.target,
            event.edge,
            event.index,
            event.placement,
        )
        .unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));

        let event_a = tree.remove_edge(5).unwrap();
        let event_b = tree.remove_edge(0).unwrap();
        // index shifted by 2 because of prior removals
        let event_c = tree.remove_edge(7).unwrap();
        // restore state in reverse order of events
        tree.insert_edge(
            event_c.source,
            event_c.target,
            event_c.edge,
            event_c.index,
            event_c.placement,
        )
        .unwrap();
        tree.insert_edge(
            event_b.source,
            event_b.target,
            event_b.edge,
            event_b.index,
            event_b.placement,
        )
        .unwrap();
        tree.insert_edge(
            event_a.source,
            event_a.target,
            event_a.edge,
            event_a.index,
            event_a.placement,
        )
        .unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));
    }
}
