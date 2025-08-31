FROM rust:1.89

WORKDIR /app
COPY . .

RUN cargo build --release --bin example-barter

EXPOSE 9001

CMD ["./target/release/example-barter"]
