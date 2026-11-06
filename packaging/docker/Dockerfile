# Servidor do Elo em contêiner. O CI copia o executável já compilado de cada
# arquitetura (amd64, arm64) para cá antes de montar a imagem.
#
#   docker run -d --name elo-server -p 21116:21116 -v elo-dados:/dados \
#     ghcr.io/stringari-consultoria/elo-server
#   docker exec elo-server elo-server --impressao --data /dados
FROM busybox:1.36 AS pasta
RUN mkdir -p /dados && chown 65532:65532 /dados

FROM gcr.io/distroless/cc-debian12:nonroot
ARG TARGETARCH
COPY --chmod=755 elo-server-${TARGETARCH} /usr/local/bin/elo-server
COPY --from=pasta --chown=65532:65532 /dados /dados
VOLUME /dados
EXPOSE 21116
ENTRYPOINT ["/usr/local/bin/elo-server"]
CMD ["--listen", "0.0.0.0:21116", "--data", "/dados", "--self-signed", "--retencao-dias", "90"]
