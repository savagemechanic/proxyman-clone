import Foundation

public struct ComposedRequest: Codable, Equatable, Sendable {
    public var scheme: String
    public var host: String
    public var port: UInt16?
    public var method: String
    public var target: String
    public var headers: [HeaderField]
    public var body: String?

    public init(
        scheme: String = "https",
        host: String = "",
        port: UInt16? = nil,
        method: String = "GET",
        target: String = "/",
        headers: [HeaderField] = [],
        body: String? = nil
    ) {
        self.scheme = scheme
        self.host = host
        self.port = port
        self.method = method
        self.target = target
        self.headers = headers
        self.body = body
    }
}

public struct ComposerPrefill: Equatable, Sendable {
    public let scheme: String
    public let host: String
    public let port: UInt16?
    public let method: String
    public let target: String
    public let headers: [HeaderField]
    public let body: String?
    public let warning: String?

    public init?(transaction: CapturedTransaction) {
        guard transaction.scheme == "http" || transaction.scheme == "https" else {
            return nil
        }

        scheme = transaction.scheme
        method = transaction.method
        target = transaction.target

        let defaultPort: UInt16 = transaction.scheme == "https" ? 443 : 80
        let hostHeader = transaction.requestHeaders
            .first { $0.name.caseInsensitiveCompare("Host") == .orderedSame }?
            .value
        let parsedAuthority = hostHeader.flatMap { Self.parseAuthority($0, defaultPort: defaultPort) }
        host = parsedAuthority?.host ?? transaction.host
        port = parsedAuthority.flatMap { $0.port == defaultPort ? nil : $0.port }

        headers = transaction.requestHeaders.filter {
            !Self.engineOwnedHeaders.contains($0.name.lowercased())
        }

        let hadExplicitContentLength = transaction.requestHeaders.contains {
            $0.name.caseInsensitiveCompare("Content-Length") == .orderedSame
        }

        if transaction.requestBodyBytes == 0 {
            body = hadExplicitContentLength ? "" : nil
            warning = nil
            return
        }

        guard let preview = transaction.requestBodyPreview,
              !preview.truncated,
              preview.capturedBytes == preview.totalBytes,
              preview.totalBytes == transaction.requestBodyBytes,
              let text = preview.text,
              text.lengthOfBytes(using: .utf8) == Int(transaction.requestBodyBytes)
        else {
            body = nil
            warning = "The original request body was not fully captured as UTF-8, so Composer loaded the request metadata without its body."
            return
        }

        body = text
        warning = nil
    }

    private static let engineOwnedHeaders: Set<String> = [
        "host",
        "content-length",
        "connection",
        "proxy-connection",
        "keep-alive",
        "transfer-encoding",
        "te",
        "trailer",
        "upgrade"
    ]

    private static func parseAuthority(_ raw: String, defaultPort: UInt16) -> (host: String, port: UInt16)? {
        let authority = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !authority.isEmpty else { return nil }

        if authority.hasPrefix("[") {
            guard let close = authority.firstIndex(of: "]") else { return nil }
            let hostStart = authority.index(after: authority.startIndex)
            let host = String(authority[hostStart..<close])
            guard !host.isEmpty else { return nil }
            let suffixStart = authority.index(after: close)
            let suffix = String(authority[suffixStart...])
            if suffix.isEmpty {
                return (host, defaultPort)
            }
            guard suffix.hasPrefix(":"),
                  let port = UInt16(suffix.dropFirst()),
                  port > 0
            else {
                return nil
            }
            return (host, port)
        }

        let colonCount = authority.reduce(into: 0) { count, character in
            if character == ":" { count += 1 }
        }
        if colonCount > 1 {
            return (authority, defaultPort)
        }
        if colonCount == 1, let colon = authority.lastIndex(of: ":") {
            let host = String(authority[..<colon])
            let portText = authority[authority.index(after: colon)...]
            guard !host.isEmpty, let port = UInt16(portText), port > 0 else { return nil }
            return (host, port)
        }
        return (authority, defaultPort)
    }
}
