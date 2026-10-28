"""Bounded read-only PTSL bridge. No import execution command is exposed."""
import json
import socket
import sys
import time
import uuid

SITE_PACKAGES = r"D:/Ai-Files/Skills/ProTools技能/pt-exporter/env/venv/Lib/site-packages"
sys.path.insert(0, SITE_PACKAGES)


def status():
    result = {"connected": False, "state": "not_listening", "endpoint": "127.0.0.1:31416", "session": None}
    try:
        with socket.create_connection(("127.0.0.1", 31416), timeout=0.5):
            pass
    except OSError:
        return result
    try:
        import grpc
        from ptsl import PTSL_pb2 as pb, PTSL_pb2_grpc
        deadline = time.monotonic() + 7
        with grpc.insecure_channel(result["endpoint"]) as channel:
            stub = PTSL_pb2_grpc.PTSLStub(channel)
            session_id = ""

            def read(command, body=None):
                # Deliberate allowlist: neither Import nor any project mutation is callable.
                allowed = {pb.HostReadyCheck, pb.RegisterConnection, pb.GetPTSLVersion,
                           pb.GetSessionName, pb.GetSessionPath, pb.GetSessionSampleRate}
                if command not in allowed:
                    raise ValueError("Read-only command rejected")
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("PTSL connection deadline exceeded")
                response = stub.SendGrpcRequest(pb.Request(header=pb.RequestHeader(
                    task_id=str(uuid.uuid4()), session_id=session_id, command=command, version=5),
                    request_body_json=json.dumps(body) if body is not None else ""),
                    timeout=min(2, remaining))
                if response.header.status != pb.Completed:
                    raise RuntimeError(response.response_error_json or pb.TaskStatus.Name(response.header.status))
                return json.loads(response.response_body_json or "{}")

            read(pb.HostReadyCheck)
            registration = read(pb.RegisterConnection, {"company_name": "local", "application_name": "sound-workbench-preview"})
            session_id = registration.get("session_id", "")
            if not session_id:
                raise RuntimeError("PTSL registration returned no session ID")
            result.update(connected=True, state="ready", version=read(pb.GetPTSLVersion))
            try:
                name = read(pb.GetSessionName).get("session_name", "")
                path = read(pb.GetSessionPath).get("session_path", {})
                rate = read(pb.GetSessionSampleRate).get("sample_rate")
                result["session"] = {"name": name, "path": path, "sampleRate": rate}
                if not name:
                    result["state"] = "no_session"
            except Exception as exc:
                result.update(state="session_unavailable", error=str(exc))
    except Exception as exc:
        result.update(state="connection_failed", error=str(exc))
    return result


def preview(request):
    from google.protobuf.json_format import MessageToDict
    from ptsl import PTSL_pb2 as pb
    destination = {"new_track": pb.MD_NewTrack, "clip_list": pb.MD_ClipList}[request["destination"]]
    location = {"session_start": pb.ML_SessionStart, "selection": pb.ML_Selection}[request["location"]]
    body = pb.ImportRequestBody(import_type=pb.Audio, audio_data=pb.AudioData(
        file_list=[item["path"] for item in request["assets"]],
        audio_operations=pb.CopyAudio, audio_destination=destination, audio_location=location,
        location_data=pb.SpotLocationData()))
    return {"dryRun": True, "executionEnabled": False, "assets": request["assets"],
            "target": status(), "request": MessageToDict(body, preserving_proto_field_name=True),
            "warnings": ["仅预览；未向 Pro Tools 发送 Import。", "未校验目标工程可写性、格式转换和实际导入兼容性。"]}


def main():
    try:
        request = json.load(sys.stdin)
        if request.get("action") == "status":
            output = status()
        elif request.get("action") == "preview":
            output = preview(request)
        else:
            raise ValueError("Unsupported read-only bridge action")
        print(json.dumps(output, ensure_ascii=True))
    except Exception as exc:
        print(json.dumps({"error": str(exc)}, ensure_ascii=True))
        sys.exit(1)


if __name__ == "__main__":
    main()
