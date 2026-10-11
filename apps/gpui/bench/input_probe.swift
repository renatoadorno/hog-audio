// Montagem e medições 3, 5, 6, 8 e 9: gera o gesto com CGEvent, lê a janela pelo CGWindowList e
// o device por Core Audio, só leitura. Nada aqui usa a API de acessibilidade, que no GPUI deixa a
// janela sem cache até fechar. Os gestos precisam da permissão de Acessibilidade para o terminal
// (Ajustes do Sistema > Privacidade e Segurança > Acessibilidade).
//
//   swiftc -O -o apps/gpui/bench/build/input_probe apps/gpui/bench/input_probe.swift
//   input_probe list
//   input_probe read <uid>
//   input_probe frame <pid>
//   input_probe slider <x1> <y1> <x2> <y2> <uid>
//   input_probe click <x> <y> <uid>
//   input_probe scroll <x> <y> <linhas> <vezes>
//   input_probe drag <x1> <y1> <x2> <y2>
//   input_probe key <keycode> [cmd] [shift] [alt] [ctrl]
//
// Saída: 0 ok; 1 device ou janela não encontrados, ou o device não parou e voltou; 2 uso;
// 3 sem permissão para postar eventos.
import CoreAudio
import CoreGraphics
import Foundation

func nowMs() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1_000_000 }

func usage() -> Never {
    print("uso: input_probe list | read <uid> | frame <pid> | slider x1 y1 x2 y2 <uid> | click x y <uid> | scroll x y linhas vezes | drag x1 y1 x2 y2 | key keycode [cmd] [shift] [alt] [ctrl]")
    exit(2)
}

func number(_ text: String) -> Double {
    guard let value = Double(text), value.isFinite else { usage() }
    return value
}

func point(_ x: String, _ y: String) -> CGPoint { CGPoint(x: number(x), y: number(y)) }

func address(_ selector: AudioObjectPropertySelector,
             _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal,
             _ element: AudioObjectPropertyElement = kAudioObjectPropertyElementMain) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: selector, mScope: scope, mElement: element)
}

func read<T: BitwiseCopyable>(_ id: AudioObjectID, _ addr: AudioObjectPropertyAddress, _ initial: T) -> T? {
    var addr = addr
    var value = initial
    var size = UInt32(MemoryLayout<T>.size)
    return AudioObjectGetPropertyData(id, &addr, 0, nil, &size, &value) == noErr ? value : nil
}

func string(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
    var addr = address(selector)
    var value: Unmanaged<CFString>?
    var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    guard AudioObjectGetPropertyData(id, &addr, 0, nil, &size, &value) == noErr else { return nil }
    return value?.takeRetainedValue() as String?
}

func devices() -> [AudioObjectID] {
    var addr = address(kAudioHardwarePropertyDevices)
    var size: UInt32 = 0
    AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size)
    var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
    AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size, &ids)
    return ids
}

func device(uid: String) -> AudioObjectID? {
    devices().first { string($0, kAudioDevicePropertyDeviceUID) == uid }
}

/// Sob hog o "default output" aponta para outro device: por isso o device vem sempre pelo UID.
func volume(_ id: AudioObjectID) -> Float32? {
    read(id, address(kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyScopeOutput), Float32(0))
        ?? read(id, address(kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyScopeOutput, 1), Float32(0))
}

func describe(_ volume: Float32?) -> String { volume.map { String($0) } ?? "?" }

func running(_ id: AudioObjectID) -> Bool? {
    read(id, address(kAudioDevicePropertyDeviceIsRunningSomewhere), UInt32(0)).map { $0 != 0 }
}

func post(_ type: CGEventType, _ point: CGPoint) {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)?
        .post(tap: .cghidEventTap)
}

/// Aperta em `from`, arrasta em 30 passos de 10 ms e solta em `to`.
func drag(from: CGPoint, to: CGPoint) {
    post(.leftMouseDown, from)
    for step in 1...30 {
        let t = Double(step) / 30
        post(.leftMouseDragged, CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t))
        usleep(10_000)
    }
    post(.leftMouseUp, to)
}

/// Sem a permissão o macOS descarta os eventos em silêncio; melhor parar antes do gesto.
func requirePostEventAccess() {
    guard CGPreflightPostEventAccess() else {
        print("sem permissão para postar eventos: dê Acessibilidade ao app de terminal (Ajustes do Sistema > Privacidade e Segurança > Acessibilidade) e reabra o terminal")
        exit(3)
    }
}

func requireDevice(_ uid: String) -> AudioObjectID {
    guard let id = device(uid: uid) else {
        print("device não encontrado: \(uid) (rode `input_probe list`)")
        exit(1)
    }
    return id
}

/// Janelas de camada 0 do processo na tela, da frente para trás.
func windows(pid: pid_t) -> [(id: CGWindowID, bounds: CGRect)] {
    guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else {
        return []
    }
    return list.compactMap { info in
        guard (info[kCGWindowOwnerPID as String] as? pid_t) == pid,
              (info[kCGWindowLayer as String] as? Int) == 0,
              let id = info[kCGWindowNumber as String] as? CGWindowID,
              let dict = info[kCGWindowBounds as String] as? NSDictionary,
              let bounds = CGRect(dictionaryRepresentation: dict as CFDictionary) else {
            return nil
        }
        return (id, bounds)
    }
}

let args = Array(CommandLine.arguments.dropFirst())
switch args.first {
case "list":
    for id in devices() {
        print("\(id)\t\(string(id, kAudioDevicePropertyDeviceUID) ?? "?")\t\(string(id, kAudioObjectPropertyName) ?? "?")")
    }
case "read" where args.count == 2:
    let id = requireDevice(args[1])
    let rate = read(id, address(kAudioDevicePropertyNominalSampleRate), Float64(0)) ?? -1
    let hog = read(id, address(kAudioDevicePropertyHogMode), pid_t(0)) ?? -2
    print("volume \(volume(id).map { String($0) } ?? "sem controle")  rodando \(running(id).map { String($0) } ?? "?")  rate \(rate)  hog \(hog)")
case "frame" where args.count == 2:
    guard let pid = pid_t(args[1]) else { usage() }
    let found = windows(pid: pid)
    guard !found.isEmpty else {
        print("nenhuma janela na tela para o pid \(pid)")
        exit(1)
    }
    for window in found {
        let b = window.bounds
        print(String(format: "janela %u  x %g  y %g  w %g  h %g", window.id, b.minX, b.minY, b.width, b.height))
    }
case "slider" where args.count == 6:
    let (from, to) = (point(args[1], args[2]), point(args[3], args[4]))
    requirePostEventAccess()
    let id = requireDevice(args[5])
    let before = volume(id)
    drag(from: from, to: to)
    let released = nowMs()
    var last = volume(id)
    var lastChange = released
    while nowMs() - released < 2_000 {
        let current = volume(id)
        if current != last {
            last = current
            lastChange = nowMs()
        }
        usleep(1_000)
    }
    print(String(format: "soltar → último valor no hardware: %.1f ms (volume antes %@, depois %@)", lastChange - released, describe(before), describe(last)))
case "click" where args.count == 4:
    let target = point(args[1], args[2])
    requirePostEventAccess()
    let id = requireDevice(args[3])
    post(.leftMouseDown, target)
    post(.leftMouseUp, target)
    let clicked = nowMs()
    var stopped = running(id) == false
    while nowMs() - clicked < 3_000 {
        let now = running(id)
        if now == false { stopped = true }
        if stopped, now == true {
            print(String(format: "clique → device rodando de novo: %.1f ms", nowMs() - clicked))
            exit(0)
        }
        usleep(500)
    }
    print("o device não parou e voltou em 3 s: o clique chegou à linha? (janela ativa, coordenada dentro da linha)")
    exit(1)
case "scroll" where args.count == 5:
    let target = point(args[1], args[2])
    guard let lines = Int32(args[3]), let times = Int(args[4]), times >= 1 else { usage() }
    requirePostEventAccess()
    post(.mouseMoved, target)
    for _ in 0..<times {
        CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: lines, wheel2: 0, wheel3: 0)?
            .post(tap: .cghidEventTap)
        usleep(16_000)
    }
case "drag" where args.count == 5:
    let (from, to) = (point(args[1], args[2]), point(args[3], args[4]))
    requirePostEventAccess()
    // Pousar o cursor antes de apertar, como a mão faz, para o canto da janela reconhecer o
    // redimensionamento.
    post(.mouseMoved, from)
    usleep(50_000)
    drag(from: from, to: to)
case "key" where args.count >= 2:
    // O evento vai para a janela-chave do app ativo: trazer o app para a frente antes.
    guard let code = CGKeyCode(args[1]) else { usage() }
    var flags: CGEventFlags = []
    for name in args.dropFirst(2) {
        switch name {
        case "cmd": flags.insert(.maskCommand)
        case "shift": flags.insert(.maskShift)
        case "alt": flags.insert(.maskAlternate)
        case "ctrl": flags.insert(.maskControl)
        default: usage()
        }
    }
    requirePostEventAccess()
    let modifiers: CGEventFlags = [.maskCommand, .maskShift, .maskAlternate, .maskControl]
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else { continue }
        // Só os quatro modificadores são trocados, para um preso no estado global não vazar;
        // as flags que o evento traz por ser seta ou tecla de função ficam.
        event.flags = event.flags.subtracting(modifiers).union(flags)
        event.post(tap: .cghidEventTap)
        usleep(10_000)
    }
default:
    usage()
}
