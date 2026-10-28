"""Offline quality report. Does not infer labels, call a model, or invent scores."""
import argparse
import json
from pathlib import Path


def evaluate(dataset, results):
    queries = dataset.get("queries")
    if not isinstance(queries, list) or not queries:
        raise ValueError("queries must be a non-empty list")
    seen = set()
    labeled = []
    for item in queries:
        text, relevant = item.get("query"), item.get("relevant_asset_ids")
        if not isinstance(text, str) or not text.strip() or text in seen:
            raise ValueError("query labels must be unique non-empty text")
        seen.add(text)
        if not isinstance(relevant, list) or any(not isinstance(v, str) or not v for v in relevant):
            raise ValueError("relevant_asset_ids must be a list of IDs")
        if relevant:
            labeled.append((text, set(relevant)))
    base = {"labeled_queries": len(labeled), "unlabeled_queries": len(queries) - len(labeled),
            "metrics": {"top_10_hit_rate": None, "mean_reciprocal_rank": None}, "per_query": []}
    if not labeled:
        return {**base, "status": "not_run_missing_labels"}
    if results.get("basis") != "audio_content":
        raise ValueError("only explicitly declared audio_content retrieval results may be scored here")
    predictions = results.get("queries", {})
    if not isinstance(predictions, dict) or any(text not in predictions for text, _ in labeled):
        return {**base, "status": "incomplete_results"}
    hits, reciprocal = 0, 0.0
    for text, relevant in labeled:
        ranking = predictions[text]
        if not isinstance(ranking, list) or len(ranking) > 100 or any(not isinstance(v, str) or not v for v in ranking):
            raise ValueError("each ranking must contain at most 100 asset IDs")
        ranking = list(dict.fromkeys(ranking))
        rank = next((i for i, asset in enumerate(ranking, 1) if asset in relevant), None)
        hit = rank is not None and rank <= 10
        hits += int(hit)
        reciprocal += 1 / rank if rank else 0
        base["per_query"].append({"query": text, "first_relevant_rank": rank, "top_10_hit": hit})
    return {**base, "status": "evaluated", "metrics": {"top_10_hit_rate": hits / len(labeled),
            "mean_reciprocal_rank": reciprocal / len(labeled)}}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("dataset", type=Path)
    parser.add_argument("results", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = evaluate(json.loads(args.dataset.read_text(encoding="utf-8-sig")),
                      json.loads(args.results.read_text(encoding="utf-8-sig")))
    payload = json.dumps(report, ensure_ascii=False, indent=2)
    if args.output:
        with args.output.open("x", encoding="utf-8") as file:
            file.write(payload + "\n")
    else:
        print(payload)


if __name__ == "__main__":
    main()
