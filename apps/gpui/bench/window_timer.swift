// Medição 4: do lançamento até a primeira janela do processo aparecer no WindowServer, pelo
// CGWindowList, igual para os dois apps. Termina o app com SIGTERM, que os dois tratam
// devolvendo o device; se ele não sair em 5 s, manda SIGKILL e registra na linha da rodada.
//
//   swiftc -O -o apps/gpui/bench/build/window_timer apps/gpui/bench/window_timer.swift
//   apps/gpui/bench/build/window_timer <executável> [rodadas]
//
// Saída: 0 ok; 1 a janela não apareceu; 2 uso; 3 o executável não lançou; 4 o app terminou
// antes de abrir a janela.
import CoreGraphics
import Foundation

let limitMs = 15_000.0
let termLimitMs = 5_000.0

func nowMs() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1_000_000 }

func windowOnScreen(pid: pid_t) -> Bool {
    guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else {
        return false
    }
    return list.contains {
        ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && ($0[kCGWindowLayer as String] as? Int) == 0
    }
}

/// SIGTERM e espera até `termLimitMs`; passado o teto, SIGKILL. Devolve se precisou do SIGKILL.
func stop(_ process: Process) -> Bool {
    guard process.isRunning else { return false }
    process.terminate()
    let sent = nowMs()
    while process.isRunning, nowMs() - sent < termLimitMs {
        usleep(5_000)
    }
    guard process.isRunning else { return false }
    kill(process.processIdentifier, SIGKILL)
    process.waitUntilExit()
    return true
}

func usage() -> Never {
    print("uso: window_timer <executável> [rodadas, a partir de 1]")
    exit(2)
}

let args = CommandLine.arguments
guard args.count == 2 || args.count == 3 else { usage() }
let executable = URL(fileURLWithPath: args[1])
let rounds = args.count == 3 ? Int(args[2]) ?? 0 : 1
guard rounds >= 1 else { usage() }

for round in 1...rounds {
    let process = Process()
    process.executableURL = executable
    let start = nowMs()
    do {
        try process.run()
    } catch {
        print("rodada \(round): não consegui lançar \(executable.path): \(error.localizedDescription)")
        exit(3)
    }
    var elapsed: Double?
    while nowMs() - start < limitMs {
        if windowOnScreen(pid: process.processIdentifier) {
            elapsed = nowMs() - start
            break
        }
        guard process.isRunning else {
            let how = process.terminationReason == .uncaughtSignal ? "sinal" : "status"
            print("rodada \(round): o app terminou antes de abrir a janela (\(how) \(process.terminationStatus))")
            exit(4)
        }
        usleep(5_000)
    }
    let killNote = stop(process) ? "; não saiu em \(Int(termLimitMs / 1000)) s com SIGTERM e levou SIGKILL" : ""
    guard let elapsed else {
        print("rodada \(round): a janela não apareceu em \(Int(limitMs / 1000)) s\(killNote)")
        exit(1)
    }
    print(String(format: "rodada %d: %.0f ms", round, elapsed) + killNote)
    sleep(2)
}
