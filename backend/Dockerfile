FROM rust:1.86

WORKDIR /app
COPY . .

RUN cargo build --release --bin ws-backend

EXPOSE 9001

CMD ["./target/release/ws-backend"]
