"""cqfusion command-line interface."""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING

from cqlib_tianyan.fusion import __version__
from cqlib_tianyan.fusion.client.models import ResourcePlan
from cqlib_tianyan.fusion.exceptions import CqfusionError, CqfusionRequestError
from cqlib_tianyan.fusion.package.builder import build_package
from cqlib_tianyan.fusion.package.loader import load_workflow_from_dir
from cqlib_tianyan.fusion.package.validator import validate_package_dir
from cqlib_tianyan.fusion.runtime.factory import (
    create_classic_runner,
    create_quantum_runner,
)
from cqlib_tianyan.fusion.runtime.recommend import recommend_local
from cqlib_tianyan.fusion.runtime.orchestrator import Orchestrator
from cqlib_tianyan.fusion.spec.manifest import load_manifest
from cqlib_tianyan.fusion.spec.profile import extract_profile

if TYPE_CHECKING:
    from cqlib_tianyan.fusion.client.fusion_client import FusionClient


def _print_json(data) -> None:
    print(json.dumps(data, ensure_ascii=False, indent=2))


def _resolve_task_dir(path: Path) -> tuple[Path, Path | None]:
    root = path.resolve()
    if root.is_file() and root.suffix == ".zip":
        tmp = Path(tempfile.mkdtemp(prefix="cqfusion-task-"))
        import zipfile

        with zipfile.ZipFile(root) as archive:
            archive.extractall(tmp)
        return tmp, root
    return root, None


def _add_dispatch_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--dispatch",
        choices=["intelligent", "manual"],
        default="intelligent",
        help="dispatch mode",
    )
    parser.add_argument(
        "--preference",
        choices=["quality", "speed", "cost", "balanced"],
        default="balanced",
        help="intelligent dispatch preference",
    )
    parser.add_argument(
        "--quantum-resource",
        default=None,
        help="required binding for manual dispatch; hint for intelligent dispatch",
    )
    parser.add_argument(
        "--hpc-resource",
        default=None,
        help="required binding for manual dispatch; hint for intelligent dispatch",
    )


def _add_cloud_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--host", default=os.environ.get("CQFUSION_HOST", "https://qc.zdxlz.com")
    )
    parser.add_argument("--token", default=os.environ.get("CQFUSION_TOKEN", ""))
    parser.add_argument("--login-key", default=None)


def _make_client(args: argparse.Namespace) -> FusionClient:
    try:
        from cqlib_tianyan.fusion.client.fusion_client import FusionClient
    except ImportError as exc:
        raise CqfusionRequestError(
            "cloud commands require the cqlib-tianyan native extension; "
            "install cqlib-tianyan"
        ) from exc
    return FusionClient(
        host=args.host,
        token=args.token,
        login_key=args.login_key or None,
    )


def _cmd_validate(args: argparse.Namespace) -> int:
    report = validate_package_dir(args.path)
    if report.profile:
        _print_json(report.profile)
    for warning in report.warnings:
        print(f"warning: {warning}", file=sys.stderr)
    if report.ok:
        print(report)
        return 0
    print(report, file=sys.stderr)
    return 1


def _cmd_build(args: argparse.Namespace) -> int:
    out = build_package(args.path, args.output)
    print(out)
    return 0


def _cmd_run_local(args: argparse.Namespace) -> int:
    root, _ = _resolve_task_dir(args.path)
    manifest = load_manifest(root / "fusion.yaml")
    workflow = load_workflow_from_dir(root)
    profile = extract_profile(manifest, workflow)
    _print_json(profile.to_dict())
    selected = _select_local_plan(args, manifest, profile)
    if selected is None:
        return 2

    workdir = Path(args.workdir)
    classic_runner = create_classic_runner(
        args.classic,
        workdir,
        slurm_partition=args.slurm_partition,
        slurm_dry_run=args.slurm_dry_run,
    )
    quantum_runner = create_quantum_runner(
        args.quantum,
        manifest,
        machine_name=args.machine or selected.raw.get("machine"),
        login_key=args.login_key or None,
        transpile=not args.no_transpile,
    )

    result = Orchestrator(
        workflow,
        manifest,
        workdir,
        classic_runner=classic_runner,
        quantum_runner=quantum_runner,
    ).run()
    _print_json(
        {
            "status": result.status,
            "round": result.context.round,
            "classic_iterations": result.context.classic_iterations,
            "quantum_iterations": result.context.quantum_iterations,
            "subtasks": result.subtasks,
            "events": len(result.events),
            "classic_backend": args.classic,
            "quantum_backend": args.quantum,
            "selected_plan": _plans_payload([selected])[0],
        }
    )
    return 0


def _prepare_package(path: Path, output: Path | None) -> Path:
    if path.is_file() and path.suffix == ".zip":
        return path
    if output is None:
        output = Path(tempfile.gettempdir()) / f"{path.name}.zip"
    return build_package(path, output)


def _select_local_plan(
    args: argparse.Namespace, manifest, profile
) -> ResourcePlan | None:
    plans = recommend_local(
        profile,
        preference=args.preference,
        quantum_hint=manifest.resources.quantum,
        hpc_hint=manifest.resources.hpc,
    )
    if not plans:
        print("没有可执行的资源组合", file=sys.stderr)
        return None
    print("TOP 3 资源组合（本地估算，选择后才开始执行）", file=sys.stderr)
    for plan in plans:
        print(
            f"  {plan.rank}. {plan.id}  {plan.quantum_resource} + {plan.hpc_resource}\n"
            f"     score={plan.score}  质量 {plan.quality}  等待 {plan.wait}  费用 {plan.cost}",
            file=sys.stderr,
        )
    chosen = _chosen_plan(plans, plan_id=args.plan, auto_confirm=args.auto_confirm)
    if chosen is not None or args.plan or args.auto_confirm:
        if chosen is None:
            return None
        print(
            f"已选择第 {chosen.rank} 名：{chosen.quantum_resource} + {chosen.hpc_resource}",
            file=sys.stderr,
        )
        return chosen
    if not sys.stdin.isatty():
        print("非交互环境请加 --auto-confirm 或 --plan PLAN_ID", file=sys.stderr)
        return None
    while True:
        answer = input("选择方案 [1-3]，直接回车取消: ").strip()
        if not answer:
            print("已取消，未开始执行", file=sys.stderr)
            return None
        chosen = _match_plan(plans, answer)
        if chosen is not None:
            print(
                f"已选择第 {chosen.rank} 名：{chosen.quantum_resource} + {chosen.hpc_resource}",
                file=sys.stderr,
            )
            return chosen
        print("没有这个方案，请输入 1、2、3 或 plan id", file=sys.stderr)


def _chosen_plan(
    plans: list[ResourcePlan],
    *,
    plan_id: str | None,
    auto_confirm: bool,
) -> ResourcePlan | None:
    if plan_id:
        chosen = _match_plan(plans, plan_id)
        if chosen is None:
            known = ", ".join(plan.id for plan in plans)
            print(f"unknown plan {plan_id!r}; available: {known}", file=sys.stderr)
        return chosen
    if auto_confirm:
        return plans[0]
    return None


def _match_plan(plans: list[ResourcePlan], answer: str) -> ResourcePlan | None:
    if answer.isdigit():
        rank = int(answer)
        for plan in plans:
            if plan.rank == rank:
                return plan
        return None
    for plan in plans:
        if plan.id == answer:
            return plan
    return None


def _plans_payload(plans: list[ResourcePlan]) -> list[dict]:
    return [
        {
            "rank": plan.rank,
            "plan_id": plan.id,
            "quantum_resource": plan.quantum_resource,
            "hpc_resource": plan.hpc_resource,
            "score": plan.score,
            "quality": plan.quality,
            "wait": plan.wait,
            "execute": plan.execute,
            "cost": plan.cost,
            "reliability": plan.reliability,
        }
        for plan in plans
    ]


def _prompt_plan(plans: list[ResourcePlan]) -> ResourcePlan | None:
    print(
        "Select a resource plan (number or plan id, empty to leave the draft):",
        file=sys.stderr,
    )
    for plan in plans:
        print(
            f"  {plan.rank}. {plan.id}  {plan.quantum_resource} + {plan.hpc_resource}"
            f"  score={plan.score or '-'}",
            file=sys.stderr,
        )
    answer = input("> ").strip()
    if not answer:
        return None
    if answer.isdigit():
        rank = int(answer)
        for plan in plans:
            if plan.rank == rank:
                return plan
    for plan in plans:
        if plan.id == answer:
            return plan
    print(f"unknown selection {answer!r}", file=sys.stderr)
    return None


def _cmd_submit(args: argparse.Namespace) -> int:
    client = _make_client(args)
    package = _prepare_package(args.path.resolve(), args.output)
    schedule = client.schedule_package(package, args.preference)
    candidates = schedule.candidates()
    _print_json(candidates)
    if args.dispatch == "manual":
        if not args.quantum_resource or not args.hpc_resource:
            raise CqfusionRequestError("manual dispatch requires both resource codes")
        decision = schedule.manual(args.quantum_resource, args.hpc_resource)
    else:
        selected = (
            next((c for c in candidates if c["group_id"] == args.plan), None)
            if args.plan
            else None
        )
        if args.auto_confirm and not args.plan:
            selected = next((c for c in candidates if c["selectable"]), None)
        if (
            selected is None
            and not args.plan
            and not args.auto_confirm
            and sys.stdin.isatty()
        ):
            answer = input("Select a recommendation rank (empty to cancel): ").strip()
            selected = next((c for c in candidates if str(c["rank"]) == answer), None)
        if selected is None:
            print(
                "No job created; select a current recommendation with --plan or --auto-confirm, or use manual resource codes",
                file=sys.stderr,
            )
            return 2
        decision = schedule.select(selected["rank"])
    handle = client.submit_decision(args.name, decision)
    _print_json(json.loads(handle.status()))
    if args.wait:
        _print_json(json.loads(handle.wait(timeout=args.timeout)))
    return 0


def _cmd_recommend(args: argparse.Namespace) -> int:
    print(
        "cqfusion recommend is deprecated: /sdk/fusion is not deployed; use cqfusion submit with the v2 job API",
        file=sys.stderr,
    )
    return 2


def _cmd_confirm(args: argparse.Namespace) -> int:
    print(
        "cqfusion confirm is deprecated: /sdk/fusion is not deployed; use cqfusion submit with the v2 job API",
        file=sys.stderr,
    )
    return 2


def _cmd_save(args: argparse.Namespace) -> int:
    print(
        "cqfusion save is deprecated: /sdk/fusion is not deployed; use cqfusion submit with the v2 job API",
        file=sys.stderr,
    )
    return 2


def _cmd_status(args: argparse.Namespace) -> int:
    client = _make_client(args)
    _print_json(client.progress(args.task_id))
    if args.events:
        _print_json(client.events(args.task_id))
    if args.subtasks:
        _print_json([item.__dict__ for item in client.subtasks(args.task_id)])
    return 0


def _cmd_download(args: argparse.Namespace) -> int:
    client = _make_client(args)
    files = [client.download_job_result(args.task_id, args.dest)]
    _print_json([str(path) for path in files])
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="cqfusion")
    parser.add_argument(
        "--version", action="version", version=f"cqfusion {__version__}"
    )
    sub = parser.add_subparsers(dest="command", required=True)

    validate = sub.add_parser("validate", help="validate a fusion task directory")
    validate.add_argument("path", type=Path)
    validate.set_defaults(func=_cmd_validate)

    build = sub.add_parser("build", help="build a fusion task zip")
    build.add_argument("path", type=Path)
    build.add_argument("-o", "--output", type=Path, required=True)
    build.set_defaults(func=_cmd_build)

    run_local = sub.add_parser("run-local", help="run workflow locally")
    run_local.add_argument("path", type=Path)
    run_local.add_argument("--workdir", type=Path, default=Path("/tmp/cqfusion-run"))
    run_local.add_argument("--classic", choices=["local", "slurm"], default="local")
    run_local.add_argument("--quantum", choices=["mock", "tianyan"], default="mock")
    run_local.add_argument(
        "--machine", default=None, help="quantum machine code for tianyan"
    )
    run_local.add_argument("--login-key", default=None)
    run_local.add_argument("--slurm-partition", default=None)
    run_local.add_argument("--slurm-dry-run", action="store_true")
    run_local.add_argument("--no-transpile", action="store_true")
    run_local.add_argument(
        "--preference",
        choices=["quality", "speed", "cost", "balanced"],
        default="balanced",
        help="how to rank the local TOP 3 resource plans",
    )
    run_local.add_argument("--plan", default=None, help="select this plan id and start")
    run_local.add_argument(
        "--auto-confirm",
        action="store_true",
        help="start with the first recommended plan without prompting",
    )
    run_local.set_defaults(func=_cmd_run_local)

    submit = sub.add_parser("submit", help="submit fusion task to cloud")
    submit.add_argument("path", type=Path, help="task directory or zip")
    submit.add_argument("--name", required=True)
    submit.add_argument(
        "-o", "--output", type=Path, help="zip output when path is directory"
    )
    submit.add_argument("--wait", action="store_true")
    submit.add_argument("--timeout", type=float, default=3600.0)
    submit.add_argument(
        "--auto-confirm",
        action="store_true",
        help="accept the first recommended plan without prompting",
    )
    submit.add_argument("--plan", default=None, help="confirm this plan id and enqueue")
    _add_dispatch_args(submit)
    _add_cloud_args(submit)
    submit.set_defaults(func=_cmd_submit)

    save = sub.add_parser("save", help="deprecated SDK save (not deployed)")
    save.add_argument("path", type=Path)
    save.add_argument("--name", required=True)
    save.add_argument("-o", "--output", type=Path)
    _add_dispatch_args(save)
    _add_cloud_args(save)
    save.set_defaults(func=_cmd_save)

    recommend = sub.add_parser(
        "recommend", help="deprecated SDK recommend (not deployed)"
    )
    recommend.add_argument("task_id")
    recommend.add_argument(
        "--preference",
        choices=["quality", "speed", "cost", "balanced"],
        default="balanced",
    )
    _add_cloud_args(recommend)
    recommend.set_defaults(func=_cmd_recommend)

    confirm = sub.add_parser("confirm", help="deprecated SDK confirm (not deployed)")
    confirm.add_argument("task_id")
    confirm.add_argument("plan_id")
    confirm.add_argument("--wait", action="store_true")
    confirm.add_argument("--timeout", type=float, default=3600.0)
    _add_cloud_args(confirm)
    confirm.set_defaults(func=_cmd_confirm)

    status = sub.add_parser("status", help="query fusion task status")
    status.add_argument("task_id")
    status.add_argument("--events", action="store_true")
    status.add_argument("--subtasks", action="store_true")
    _add_cloud_args(status)
    status.set_defaults(func=_cmd_status)

    download = sub.add_parser("download", help="download fusion task results")
    download.add_argument("task_id")
    download.add_argument("dest", type=Path)
    _add_cloud_args(download)
    download.set_defaults(func=_cmd_download)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except (CqfusionError, CqfusionRequestError) as exc:
        print(str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
