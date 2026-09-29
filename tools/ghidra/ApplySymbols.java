// Ghidra script: apply docs/engine/symbols.csv (address,name,kind,comment) to the current program, so the
// decompile shows our names. Functions are created if missing; comments go on as plate comments.
// Args: <symbols.csv>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

public class ApplySymbols extends GhidraScript {
    @Override
    public void run() throws Exception {
        List<String> lines = Files.readAllLines(Path.of(getScriptArgs()[0]));
        int n = 0;
        for (String line : lines.subList(1, lines.size())) {
            String[] f = line.split(",", 4);
            if (f.length < 3) continue;
            Address a = toAddr(Long.decode(f[0].trim()));
            String name = f[1].trim();
            if (f[2].trim().equals("function")) {
                Function fn = getFunctionAt(a);
                if (fn == null) {
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
        println("applied " + n + " symbols");
    }
}
