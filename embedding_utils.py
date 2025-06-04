from observed_dict_rs import Vertex


def attach_embeddings_from_meta(vertex: Vertex) -> None:
    """Propagate embeddings stored in ``vertex.meta`` to their nodes.

    The vertex must have ``embedding`` and ``embedding_ids`` entries in
    ``vertex.meta``. They are typically collected via a callback when
    nodes are added.
    """
    embeddings = vertex.meta.get("embedding")
    node_ids = vertex.meta.get("embedding_ids")
    if embeddings is None or node_ids is None:
        raise ValueError("vertex.meta must contain 'embedding' and 'embedding_ids'")

    for e, node_id in zip(embeddings, node_ids):
        node = vertex.get_node(node_id)
        node.attr_list_append("embeddings", e)
