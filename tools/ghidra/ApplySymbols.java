// Ghidra script: apply docs/engine/symbols.csv (address,name,kind,comment) to the current program, so the
// decompile shows our names. Functions are created if missing; comments go on as plate comments.
// Instruction set (TMode) of each function, most reliable signal first: IWRAM (0x03xxxxxx) is ARM; else the
// mode of a caller (a Thumb `bl` keeps the mode); else Thumb if the ROM holds a Thumb pointer (address | 1) to
// it, as the handler tables do; else the mode of the instruction before it. An existing function in the wrong
// mode is cleared and redone.
// Args: <symbols.csv>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.lang.Register;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.ProgramContext;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.SourceType;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

public class ApplySymbols extends GhidraScript {
    private ProgramContext context;
    private Register tmode;

    private BigInteger wantedMode(long offset, Address a) throws Exception {
        if ((offset >>> 24) == 0x03) return BigInteger.ZERO;
        // Only direct branches tell the mode: `bl` keeps the caller's, `blx #imm` flips it. Register jumps
        // (`bx rN`, `ldr pc`, `blx rN`) take it from the target address's bit 0, handled by the pointer search below.
        for (Reference r : getReferencesTo(a)) {
            Instruction from = getInstructionAt(r.getFromAddress());
            if (!r.getReferenceType().isCall() || from == null || from.getNumOperands() != 1
                    || from.getOperandType(0) == ghidra.program.model.lang.OperandType.REGISTER) continue;
            String op = from.getMnemonicString().toLowerCase();
            BigInteger caller = context.getValue(tmode, r.getFromAddress(), false);
            if (caller == null) caller = BigInteger.ZERO;
            if (op.equals("bl")) return caller;
            if (op.equals("blx")) return BigInteger.ONE.subtract(caller);
        }
        long thumb = offset | 1;
        byte[] le = {(byte) thumb, (byte) (thumb >> 8), (byte) (thumb >> 16), (byte) (thumb >> 24)};
        if (find(currentProgram.getMinAddress(), le) != null) return BigInteger.ONE;
        Instruction prev = getInstructionBefore(a);
        return prev == null ? null : context.getValue(tmode, prev.getAddress(), false);
    }

    @Override
    public void run() throws Exception {
        List<String> lines = Files.readAllLines(Path.of(getScriptArgs()[0]));
        context = currentProgram.getProgramContext();
        tmode = currentProgram.getRegister("TMode");
        int n = 0, redone = 0;
        for (String line : lines.subList(1, lines.size())) {
            String[] f = line.split(",", 4);
            if (f.length < 3) continue;
            long offset = Long.decode(f[0].trim());
            Address a = toAddr(offset);
            String name = f[1].trim();
            if (f[2].trim().equals("function")) {
                BigInteger want = wantedMode(offset, a);
                Function fn = getFunctionAt(a);
                BigInteger have = context.getValue(tmode, a, false);
                if (have == null) have = BigInteger.ZERO; // no context value: Ghidra decodes ARM
                if (want != null && !want.equals(have)) {
                    if (fn != null) {
                        clearListing(fn.getBody());
                        removeFunction(fn);
                        fn = null;
                    } else {
                        clearListing(a, a.add(3));
                    }
                    redone++;
                }
                if (fn == null) {
                    if (want != null) context.setValue(tmode, a, a, want);
                    disassemble(a);
                    fn = createFunction(a, name);
                }
                if (fn == null) {
                    println("could not create function at " + a);
                    continue;
                }
                fn.setName(name, SourceType.USER_DEFINED);
                if (f.length > 3) fn.setComment(f[3].trim());
            } else {
                createLabel(a, name, true, SourceType.USER_DEFINED);
            }
            n++;
        }
        println("applied " + n + " symbols; " + redone + " functions redone in the right instruction set");
    }
}
