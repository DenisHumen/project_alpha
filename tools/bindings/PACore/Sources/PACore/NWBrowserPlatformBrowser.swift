// SPDX-License-Identifier: MPL-2.0
// NET-020: the core's PlatformBrowser on iOS, iPadOS and tvOS (ADR-0019).
import Foundation
import Network

/// Bonjour browsing for the core through `NWBrowser`.
///
/// Raw multicast on iOS and tvOS needs an entitlement Apple grants by hand;
/// `NWBrowser` only needs `NSLocalNetworkUsageDescription` and the types of
/// `bonjourServices()` in `NSBonjourServices` (Info.plist). The first browse
/// shows the Local Network permission prompt.
///
/// `NWBrowser` gives the instance name and TXT record. The address comes
/// from a short TCP connection to the service endpoint (Apple's way to
/// resolve without `NetService`): the service is reported again with the
/// address and port once it answers, within ``resolveTimeout``.
///
/// ```swift
/// let discovery = LanDiscovery(browser: NWBrowserPlatformBrowser(), ownIds: [])
/// try discovery.listen(listener: myListener)
/// try discovery.browse(kinds: discoverySourceKinds())
/// ```
public final class NWBrowserPlatformBrowser: PlatformBrowser, @unchecked Sendable {
    /// How long a resolving connection may take.
    public let resolveTimeout: TimeInterval

    private let queue = DispatchQueue(label: "app.kinolune.discovery")
    private let lock = NSLock()
    private var browsers: [String: NWBrowser] = [:]
    /// Resolving connections by `"<type>|<instance>"`.
    private var resolving: [String: NWConnection] = [:]

    public init(resolveTimeout: TimeInterval = 5) {
        self.resolveTimeout = resolveTimeout
    }

    deinit {
        for browser in browsers.values { browser.cancel() }
        for connection in resolving.values { connection.cancel() }
    }

    public func startBrowse(serviceType: String, sink: PlatformSink) throws {
        let parameters = NWParameters()
        parameters.includePeerToPeer = false
        let browser = NWBrowser(
            for: .bonjourWithTXTRecord(type: serviceType, domain: nil),
            using: parameters
        )
        browser.browseResultsChangedHandler = { [weak self] _, changes in
            self?.handle(changes, serviceType: serviceType, sink: sink)
        }
        browser.stateUpdateHandler = { state in
            switch state {
            case let .failed(error), let .waiting(error):
                // Local Network permission denied, or no network: the list
                // stays empty; nothing to report to the core.
                NSLog("Kinolune discovery %@: %@", serviceType, "\(error)")
            default:
                break
            }
        }
        let previous = locked { () -> NWBrowser? in
            let old = browsers[serviceType]
            browsers[serviceType] = browser
            return old
        }
        previous?.cancel()
        browser.start(queue: queue)
    }

    public func stopBrowse(serviceType: String) {
        let (browser, connections) = locked { () -> (NWBrowser?, [NWConnection]) in
            let prefix = serviceType + "|"
            let keys = resolving.keys.filter { $0.hasPrefix(prefix) }
            return (browsers.removeValue(forKey: serviceType), keys.compactMap { resolving.removeValue(forKey: $0) })
        }
        browser?.cancel()
        connections.forEach { $0.cancel() }
    }

    private func locked<T>(_ body: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }

    private func handle(_ changes: Set<NWBrowser.Result.Change>, serviceType: String, sink: PlatformSink) {
        for change in changes {
            switch change {
            case let .added(result):
                found(result, serviceType: serviceType, sink: sink)
            case let .changed(old: _, new: result, flags: _):
                found(result, serviceType: serviceType, sink: sink)
            case let .removed(result):
                guard case let .service(name, _, _, _) = result.endpoint else { continue }
                cancelResolve(key: "\(serviceType)|\(name)")
                sink.serviceLost(instance: name)
            default:
                break
            }
        }
    }

    private func found(_ result: NWBrowser.Result, serviceType: String, sink: PlatformSink) {
        guard case let .service(name, _, _, _) = result.endpoint else { return }
        let txt = Self.txtEntries(result.metadata)
        sink.serviceFound(service: PlatformService(instance: name, host: "", port: 0, addresses: [], txt: txt))
        resolve(result.endpoint, key: "\(serviceType)|\(name)") { host, port in
            let address = Self.address(host)
            sink.serviceFound(service: PlatformService(
                instance: name,
                host: address == nil ? Self.string(host) : "",
                port: port,
                addresses: address.map { [$0] } ?? [],
                txt: txt
            ))
        }
    }

    private func resolve(
        _ endpoint: NWEndpoint,
        key: String,
        done: @escaping (NWEndpoint.Host, UInt16) -> Void
    ) {
        let connection = NWConnection(to: endpoint, using: .tcp)
        let previous = locked { () -> NWConnection? in
            let old = resolving[key]
            resolving[key] = connection
            return old
        }
        previous?.cancel()
        connection.stateUpdateHandler = { [weak self, weak connection] state in
            guard let self, let connection else { return }
            switch state {
            case .ready:
                if case let .hostPort(host, port)? = connection.currentPath?.remoteEndpoint {
                    done(host, port.rawValue)
                }
                self.cancelResolve(key: key, only: connection)
            case .failed, .waiting:
                self.cancelResolve(key: key, only: connection)
            default:
                break
            }
        }
        connection.start(queue: queue)
        queue.asyncAfter(deadline: .now() + resolveTimeout) { [weak self, weak connection] in
            guard let connection else { return }
            self?.cancelResolve(key: key, only: connection)
        }
    }

    /// Forgets the resolving connection of `key` (only if it is `only`,
    /// when given) and cancels it.
    private func cancelResolve(key: String, only: NWConnection? = nil) {
        let current = locked { () -> NWConnection? in
            guard let current = resolving[key], only == nil || current === only else { return nil }
            resolving[key] = nil
            return current
        }
        current?.cancel()
        only?.cancel()
    }

    /// TXT entries of a Bonjour result.
    static func txtEntries(_ metadata: NWBrowser.Result.Metadata) -> [TxtEntry] {
        guard case let .bonjour(record) = metadata else { return [] }
        return record.dictionary
            .map { TxtEntry(key: $0.key, value: Data($0.value.utf8)) }
            .sorted { $0.key < $1.key }
    }

    /// An IP address without the IPv6 zone, `nil` for a host name.
    static func address(_ host: NWEndpoint.Host) -> String? {
        switch host {
        case let .ipv4(a):
            return "\(a)"
        case let .ipv6(a):
            return "\(a)".split(separator: "%").first.map(String.init)
        default:
            return nil
        }
    }

    /// A host as text (`nas.local`, `192.168.1.2`).
    static func string(_ host: NWEndpoint.Host) -> String {
        if case let .name(name, _) = host { return name }
        return address(host) ?? "\(host)"
    }
}
