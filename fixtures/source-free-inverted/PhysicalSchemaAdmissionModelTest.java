// Standalone source-free invariant model. Contains NO Oreslang private source.
// Model evidence only: cannot certify oreslang-source.java#2146.
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

public final class PhysicalSchemaAdmissionModelTest {
    record Field(String name, String kind, int width, int offset) {}
    record Schema(String className, List<Field> fields) {
        Schema { fields = List.copyOf(fields); }
        List<String> fingerprint() {
            return fields.stream()
                    .map(f -> f.name() + ":" + f.kind() + ":" + f.width()
                            + ":" + f.offset())
                    .toList();
        }
    }
    record Admission(String nominalClass, String field, List<String> fingerprint) {
        Admission { fingerprint = List.copyOf(fingerprint); }
    }
    static final class Store {
        final Schema schema;
        final List<Object> column = new ArrayList<>();
        int reads;
        int writes;
        Store(Schema schema, Object initial) {
            this.schema = schema;
            column.add(initial);
        }
        Object readColumn(int row) { reads++; return column.get(row); }
        void writeColumn(int row, Object value) { writes++; column.set(row, value); }
        Object peek() { return column.get(0); }
    }
    static void admit(Store store, Admission admission) {
        if (!store.schema.className().equals(admission.nominalClass())
                || !Objects.equals(store.schema.fingerprint(), admission.fingerprint())) {
            throw new IllegalStateException("physical-schema fingerprint mismatch before access");
        }
    }
    static Object admittedRead(Store store, Admission admission) {
        admit(store, admission);
        return store.readColumn(0);
    }
    static void admittedWrite(Store store, Admission admission, Object value) {
        admit(store, admission);
        store.writeColumn(0, value);
    }
    static Object genericIndex(Store store, int row) {
        throw new IllegalStateException("generic indexing has no row capability");
    }
    static void genericWrite(Store store, int row, Object value) {
        throw new IllegalStateException("generic assignment has no row capability");
    }
    static void reject(Runnable test, String expectedMessage) {
        try {
            test.run();
            throw new AssertionError("expected rejection: " + expectedMessage);
        } catch (IllegalStateException denied) {
            if (!denied.getMessage().contains(expectedMessage)) throw denied;
        }
    }
    static Schema schema(Field... fields) {
        return new Schema("Particle", List.of(fields));
    }
    public static void main(String[] args) {
        Schema checked = schema(
                new Field("x", "f64", 8, 0), new Field("y", "f64", 8, 8));
        Admission admission = new Admission("Particle", "x", checked.fingerprint());
        Store valid = new Store(checked, 1.5d);
        if (!admittedRead(valid, admission).equals(1.5d))
            throw new AssertionError("valid read");
        admittedWrite(valid, admission, 2.5d);
        if (!valid.peek().equals(2.5d)) throw new AssertionError("valid write");

        List<Schema> substitutions = List.of(
                schema(new Field("x", "i64", 8, 0), new Field("y", "f64", 8, 8)),
                schema(new Field("y", "f64", 8, 0), new Field("x", "f64", 8, 8)),
                schema(new Field("x", "f64", 8, 0), new Field("y", "f64", 8, 16)),
                schema(new Field("x", "f64", 8, 0), new Field("y", "f64", 4, 8))
        );
        for (Schema altered : substitutions) {
            Store store = new Store(altered, 42);
            reject(() -> admittedRead(store, admission), "fingerprint mismatch");
            reject(() -> admittedWrite(store, admission, 77), "fingerprint mismatch");
            if (store.reads != 0 || store.writes != 0 || !store.peek().equals(42))
                throw new AssertionError("invalid substitution reached a column");
        }

        Store generic = new Store(checked, 4.0d);
        reject(() -> genericIndex(generic, 0), "no row capability");
        reject(() -> genericWrite(generic, 0, 7.0d), "no row capability");
        if (generic.reads != 0 || generic.writes != 0 || !generic.peek().equals(4.0d))
            throw new AssertionError("generic dispatch reached a column");
        System.out.println("source-free physical-schema admission model: 12 checks passed");
    }
}
